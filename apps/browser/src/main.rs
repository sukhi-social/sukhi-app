// On Windows do NOT show a console window when opening the app
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]
#![allow(clippy::collapsible_if)]

//! A web browser with UI powered by Dioxus Native and content rendering powered by Blitz

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::sync::Arc;

use blitz_traits::net::{Request, Url};
use dioxus_native::{NodeHandle, WindowAttributes, prelude::*, use_back_button};

#[cfg(target_os = "macos")]
use winit::platform::macos::WindowAttributesMacOS;

pub(crate) type StdNetProvider = blitz_net::Provider;

mod about_pages;
mod browser_history;
#[cfg(any(feature = "screenshot", feature = "capture"))]
mod capture;
mod document_loader;
mod favicon;
mod fps_overlay;
mod history;
mod icons;
mod lobby;
mod login;
mod nav;
mod status_bar;
mod tab;
mod tab_strip;
mod toolbar;
mod url_suggestions;

use about_pages::AboutPage;
use history::HistoryNav;
use browser_history::{BrowsingHistory, HistoryService, HistoryStore, MAX_HISTORY_ENTRIES};
use status_bar::StatusBar;
use tab::{Tab, TabId, TabStoreImplExt, TabWebView, active_tab, open_tab, tab_display_title};
use tab_strip::TabStrip;
use toolbar::Toolbar;
use url_suggestions::provide_url_suggester;
#[cfg(target_os = "windows")]
use winit::platform::windows::WinIcon;

static BROWSER_UI_STYLES: Asset = asset!("../assets/browser.css");
/// sukhi shell: one full-bleed page, no tab strip / URL bar / status bar.
/// Always on in the Android build; `SUKHI_SHELL=1` turns it on elsewhere for trying it out.
pub(crate) fn shell_mode() -> bool {
    cfg!(target_os = "android") || std::env::var_os("SUKHI_SHELL").is_some()
}
pub(crate) const IS_MOBILE: bool = cfg!(any(target_os = "android", target_os = "ios"));

#[unsafe(no_mangle)]
#[cfg(target_os = "android")]
pub fn android_main(android_app: dioxus_native::AndroidApp) {
    if let Some(dir) = android_app.internal_data_path() {
        let _ = SHELL_FILES_DIR.set(dir);
    }
    dioxus_native::set_android_app(android_app);
    main()
}

/// sukhi shell: where the Kotlin activity drops a returning URL (login callback).
#[cfg(target_os = "android")]
pub(crate) static SHELL_FILES_DIR: std::sync::OnceLock<std::path::PathBuf> =
    std::sync::OnceLock::new();
/// sukhi shell: the last URL handed back to the shell (probe: shown on about:shell-ping).
/// A Signal, so the page that shows it re-renders when a new one arrives.
pub(crate) static SHELL_CALLBACK: GlobalSignal<String> = Signal::global(String::new);

/// sukhi shell: the space the system takes (dp), as the Kotlin activity reports it.
/// `top,right,bottom,left, keyboard, corner radii TL,TR,BR,BL`
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Insets {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
    keyboard: f32,
    corners: [f32; 4],
}

impl Insets {
    /// Before the first report: the old fixed guess.
    const FALLBACK: Insets = Insets {
        top: 30.0,
        right: 0.0,
        bottom: 44.0,
        left: 0.0,
        keyboard: 0.0,
        corners: [0.0; 4],
    };

    fn parse(s: &str) -> Option<Insets> {
        let v: Vec<f32> = s
            .trim()
            .split(',')
            .map(|x| x.trim().parse::<f32>())
            .collect::<std::result::Result<_, _>>()
            .ok()?;
        if v.len() != 9 {
            return None;
        }
        Some(Insets {
            top: v[0],
            right: v[1],
            bottom: v[2],
            left: v[3],
            keyboard: v[4],
            corners: [v[5], v[6], v[7], v[8]],
        })
    }

    /// The frame's padding: the bars, and the keyboard when it is taller than the bottom bar.
    fn style(&self) -> String {
        let bottom = self.bottom.max(self.keyboard);
        format!(
            "--safe-area-top: {t}px; --safe-area-right: {r}px; --safe-area-bottom: {b}px; --safe-area-left: {l}px; \
             --safe-corner-tl: {c0}px; --safe-corner-tr: {c1}px; --safe-corner-br: {c2}px; --safe-corner-bl: {c3}px; \
             padding: {t}px {r}px {b}px {l}px;",
            t = self.top,
            r = self.right,
            b = bottom,
            l = self.left,
            c0 = self.corners[0],
            c1 = self.corners[1],
            c2 = self.corners[2],
            c3 = self.corners[3]
        )
    }
}

pub(crate) static INSETS: GlobalSignal<Insets> = Signal::global(|| Insets::FALLBACK);

#[derive(Clone)]
pub(crate) struct CliInitialUrl(pub Option<Url>);

fn parse_cli_url(s: &str) -> Option<Url> {
    if let Ok(url) = Url::parse(s) {
        return Some(url);
    }
    if s.contains('.') && !s.contains(' ') {
        if let Ok(url) = Url::parse(&format!("https://{s}")) {
            return Some(url);
        }
    }
    None
}

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();

    let cli_url = std::env::args().skip(1).find_map(|a| parse_cli_url(&a));

    let window_attributes = WindowAttributes::default();
    #[cfg(target_os = "windows")]
    let window_attributes = window_attributes
        .with_window_icon(WinIcon::from_resource(32512, None).map(Into::into).ok());
    #[cfg(target_os = "macos")]
    let window_attributes = window_attributes.with_platform_attributes(Box::new(
        WindowAttributesMacOS::default()
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
            .with_title_hidden(true)
            .with_unified_titlebar(true),
    ));

    let initial_url_ctx = CliInitialUrl(cli_url);
    let contexts: Vec<Box<dyn Fn() -> Box<dyn std::any::Any> + Send + Sync>> =
        vec![Box::new(move || {
            Box::new(initial_url_ctx.clone()) as Box<dyn std::any::Any>
        })];

    // The toolbar handles the devtools shortcuts itself, to inspect the active
    // tab rather than the browser UI
    let config = dioxus_native::Config::new()
        .with_window_attributes(window_attributes)
        .with_devtools_shortcuts([]);

    dioxus_native::launch_cfg(app, contexts, vec![Box::new(config)])
}

fn app() -> Element {
    let home_url = use_hook(|| AboutPage::NewTab.parsed_url());
    let cli_initial_url = use_hook(|| try_consume_context::<CliInitialUrl>().and_then(|c| c.0));
    let net_provider = use_context::<Arc<StdNetProvider>>();

    let url_input_handle: Signal<Option<NodeHandle>> = use_signal(|| None);
    let url_input_value = use_signal(|| home_url.to_string());

    let history_store: HistoryStore = use_hook(HistoryStore::open);

    // Synchronous on purpose: the toolbar's URL suggestions read from this
    // store on first render, so the entries need to be present before the
    // tree mounts. The read is a single sqlite query capped at
    // MAX_HISTORY_ENTRIES rows and runs once per process.
    let browsing_history: Store<BrowsingHistory> = {
        let history_store = history_store.clone();
        use_store(move || {
            BrowsingHistory::from_entries(history_store.load_recent(MAX_HISTORY_ENTRIES))
        })
    };

    let history_service = HistoryService::new(browsing_history, history_store);
    use_context_provider(|| history_service.clone());
    provide_url_suggester(browsing_history);

    let tabs: Store<Vec<Tab>> = use_store(Vec::new);
    let mut active_tab_id: Signal<TabId> = use_hook(|| {
        let first_tab_url = cli_initial_url.clone().unwrap_or_else(|| {
            if shell_mode() {
                let dir = lobby_dir();
                let origin = lobby::origin(&dir);
                // Logged in: put the saved session in the cookie jar before the first load.
                // Not yet: start on the shell's own login page.
                match login::saved_sid(&dir) {
                    Some(sid) => {
                        login::inject_cookie(&origin, &sid);
                        Url::parse(&origin).unwrap_or_else(|_| home_url.clone())
                    }
                    None => Url::parse("about:shell-login").unwrap_or_else(|_| home_url.clone()),
                }
            } else {
                home_url.clone()
            }
        });
        let tab = open_tab(tabs, first_tab_url, net_provider.clone());
        Signal::new(tab.tab_id())
    });

    let open_new_tab = use_callback(move |url: Url| {
        let new_id = open_tab(tabs, url, net_provider.clone());
        active_tab_id.set(new_id.tab_id());
        if let Some(handle) = url_input_handle() {
            drop(handle.set_focus(true));
        }
    });

    // Navigate back in the active tab's history when the (Android) hardware
    // back button is pressed. `go_back` is a no-op when there is no history to
    // go back to.
    // In the shell, back walks the stack that `history::shell_navigate` keeps (a bottom-bar
    // place sits on top of Home); at the bottom, the app goes to the background.
    use_back_button(move || {
        let tab = active_tab(tabs, active_tab_id());
        if shell_mode() && !tab.nav_history().has_back() {
            leave_to_background();
        } else {
            tab.go_back();
        }
    });

    // sukhi shell probe: pick up a returning URL that the Kotlin activity wrote to a file.
    #[cfg(target_os = "android")]
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let Some(dir) = SHELL_FILES_DIR.get() else {
                continue;
            };
            let file = dir.join("shell-callback.txt");
            if let Ok(got) = std::fs::read_to_string(&file) {
                let _ = std::fs::remove_file(&file);
                if got.starts_with("social.sukhi.shell://callback") {
                    let dir = lobby_dir();
                    let origin = lobby::origin(&dir);
                    let tab = active_tab(tabs, active_tab_id());
                    dioxus_core::spawn_forever(async move {
                        if login::complete(&dir, &origin, got.trim()).await {
                            if let Ok(url) = Url::parse(&origin) {
                                tab.navigate(Request::get(url));
                            }
                        }
                    });
                    continue;
                }
                *SHELL_CALLBACK.write() = got;
                // A fresh query each time: navigating to the same URL does not re-render the page.
                static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if let Ok(url) = Url::parse(&format!("about:shell-ping?n={n}")) {
                    active_tab(tabs, active_tab_id()).navigate(Request::get(url));
                }
            }
        }
    });

    // sukhi shell: pick up the system bars / keyboard / corners the Kotlin activity wrote.
    #[cfg(target_os = "android")]
    use_future(move || async move {
        let mut last = String::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let Some(dir) = SHELL_FILES_DIR.get() else {
                continue;
            };
            let Ok(got) = std::fs::read_to_string(dir.join("shell-insets.txt")) else {
                continue;
            };
            if got == last {
                continue;
            }
            if let Some(insets) = Insets::parse(&got) {
                *INSETS.write() = insets;
            }
            last = got;
        }
    });

    // sukhi shell: at launch, look for a newer release and (when there is one) open the lobby.
    #[cfg(target_os = "android")]
    use_future(move || async move {
        // The files dir is set in android_main; wait a moment for it.
        for _ in 0..20 {
            if let Some(dir) = SHELL_FILES_DIR.get() {
                lobby::at_launch(dir.clone()).await;
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });

    let show_fps: Signal<bool> = use_signal(|| false);

    let fps_overlay_el = rsx!(if show_fps() {
        fps_overlay::FpsOverlay {}
    });

    let frame_style = if cfg!(target_os = "android") {
        INSETS().style()
    } else {
        String::new()
    };

    let window_title = tab_display_title(active_tab(tabs, active_tab_id()));

    rsx!(
        div {
            id: "frame",
            style: "{frame_style}",
            class: if shell_mode() { "mobile shell" } else if IS_MOBILE { "mobile" } else { "" },
            onmounted: set_devtools_title,
            title { "{window_title}" }
            document::Link { rel: "stylesheet", href: BROWSER_UI_STYLES }
            if !shell_mode() {
                TabStrip {
                    tabs,
                    active_tab_id,
                    home_url,
                    open_new_tab,
                }
                Toolbar {
                    url_input_handle,
                    url_input_value,
                    tabs,
                    active_tab_id,
                    open_new_tab,
                    show_fps,
                }
            }
            for tab in tabs.iter() {
                TabWebView {
                    key: "{tab.tab_id()}",
                    tabs,
                    tab_id: tab.tab_id(),
                    active_tab_id,
                }
            }
            {fps_overlay_el}
            if !lobby::NOTICE().is_empty() {
                div { class: "lobby-notice", "{lobby::NOTICE()}" }
            }
            if let lobby::Lobby::Ready(rel) = lobby::LOBBY() {
                lobby::LobbyView { rel, dir: lobby_dir() }
            }
            if !shell_mode() {
                StatusBar { tabs, active_tab_id }
            }
        }
    )
}

/// The login page's button: open the system browser on the login page.
pub(crate) fn start_login() {
    let dir = lobby_dir();
    login::start(&dir, &lobby::origin(&dir));
}

/// Ask the Kotlin activity (it watches this file) to move the app to the background.
fn leave_to_background() {
    let dir = lobby_dir();
    if !dir.as_os_str().is_empty() {
        let _ = std::fs::write(dir.join("shell-exit.txt"), "1");
    }
}

#[cfg(target_os = "android")]
fn lobby_dir() -> std::path::PathBuf {
    SHELL_FILES_DIR.get().cloned().unwrap_or_default()
}
#[cfg(not(target_os = "android"))]
fn lobby_dir() -> std::path::PathBuf {
    std::path::PathBuf::new()
}

/// Name the browser UI's document in devtools: its `<title>` is the active
/// tab's title (for the window titlebar), which would make it look like the page
#[allow(unused_variables)]
fn set_devtools_title(evt: Event<MountedData>) {
    #[cfg(feature = "devtools")]
    if let Some(handle) = evt.downcast::<NodeHandle>() {
        handle
            .doc_mut()
            .set_devtools_title(Some("Blitz Browser UI".to_string()));
    }
}
