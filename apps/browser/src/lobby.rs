//! sukhi shell: the update lobby.
//!
//! At launch the shell asks `{origin}/_app/manifest.json` which release is current. When it is
//! newer than what this device has accepted, the release is downloaded quietly first, then one
//! full-screen page tells the person what is new and offers two choices:
//!   - いまアップデートする
//!   - ダウンロードだけして、つぎに開くときアップデートする
//! The choice is per device and lives in `lobby.json` next to the shell's other private files.
//! Wording says what happens, in plain words (no metaphors).

use std::path::{Path, PathBuf};
use std::time::Duration;

use dioxus_native::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, PartialEq, Debug, Deserialize)]
pub struct Release {
    pub version: String,
    /// Where the release is fetched from, relative to the origin.
    pub bytes_url: String,
    pub sha256: String,
    /// 新しくなったところ
    #[serde(default)]
    pub new: Vec<String>,
    /// 直したところ
    #[serde(default)]
    pub fixed: Vec<String>,
    /// 先に直したところ: security / data-safety fixes that are already live and never wait.
    #[serde(default)]
    pub urgent_fixed: Vec<String>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Lobby {
    Hidden,
    Ready(Release),
}

pub static LOBBY: GlobalSignal<Lobby> = Signal::global(|| Lobby::Hidden);
/// A one-line note shown for a few seconds after an update was applied.
pub static NOTICE: GlobalSignal<String> = Signal::global(String::new);

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    /// The release this device has accepted (the one the shell runs as).
    accepted: String,
    /// Downloaded and chosen "つぎに開くとき": applied at the next launch.
    pending: Option<String>,
}

fn saved_path(dir: &Path) -> PathBuf {
    dir.join("lobby.json")
}

fn load(dir: &Path) -> Saved {
    std::fs::read_to_string(saved_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn store(dir: &Path, saved: &Saved) {
    if let Ok(s) = serde_json::to_string(saved) {
        let _ = std::fs::write(saved_path(dir), s);
    }
}

fn release_file(dir: &Path, version: &str) -> PathBuf {
    // The version comes from the server: keep it to a safe file name.
    let safe: String = version
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    dir.join("releases").join(format!("{safe}.bin"))
}

/// Where the shell talks to. A file lets a debug build point at a mock; otherwise sukhi.social.
pub(crate) fn origin(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("shell-origin.txt"))
        .map(|s| s.trim().trim_end_matches('/').to_owned())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://sukhi.social".to_owned())
}

async fn fetch_release(origin: &str) -> Option<Release> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?;
    let text = client
        .get(format!("{origin}/_app/manifest.json"))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()?;
    serde_json::from_str(&text).ok()
}

/// Download the release and check its hash. Quiet: nothing is shown while this runs.
async fn download(origin: &str, dir: &Path, rel: &Release) -> bool {
    let file = release_file(dir, &rel.version);
    if let Ok(bytes) = std::fs::read(&file) {
        if hex(&Sha256::digest(&bytes)) == rel.sha256 {
            return true; // already here from an earlier launch
        }
    }
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
    else {
        return false;
    };
    let url = format!("{origin}{}", rel.bytes_url);
    let Ok(resp) = client.get(url).send().await else {
        return false;
    };
    let Ok(resp) = resp.error_for_status() else {
        return false;
    };
    let Ok(bytes) = resp.bytes().await else {
        return false;
    };
    if hex(&Sha256::digest(&bytes)) != rel.sha256 {
        return false; // never keep something that does not match
    }
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&file, &bytes).is_ok()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn say(text: String) {
    *NOTICE.write() = text;
    tokio::time::sleep(Duration::from_secs(6)).await;
    *NOTICE.write() = String::new();
}

/// Runs once at launch.
pub async fn at_launch(dir: PathBuf) {
    let mut saved = load(&dir);
    let origin = origin(&dir);
    let Some(rel) = fetch_release(&origin).await else {
        return; // offline or no manifest: the shell just opens
    };
    if rel.version == saved.accepted {
        return;
    }
    if saved.pending.as_deref() == Some(rel.version.as_str()) && download(&origin, &dir, &rel).await
    {
        // Chosen last time: apply it now, no second question.
        saved.accepted = rel.version.clone();
        saved.pending = None;
        store(&dir, &saved);
        say(format!(
            "前回えらんだとおり、アップデートしました(バージョン {})",
            rel.version
        ))
        .await;
        return;
    }
    if saved.accepted.is_empty() {
        // First run on this device: nothing to compare with, just remember this release.
        saved.accepted = rel.version;
        store(&dir, &saved);
        return;
    }
    if download(&origin, &dir, &rel).await {
        *LOBBY.write() = Lobby::Ready(rel);
    }
}

fn choose(dir: &Path, rel: &Release, now: bool) {
    let mut saved = load(dir);
    if now {
        saved.accepted = rel.version.clone();
        saved.pending = None;
    } else {
        saved.pending = Some(rel.version.clone());
    }
    store(dir, &saved);
}

#[component]
pub fn LobbyView(rel: Release, dir: PathBuf) -> Element {
    let version = rel.version.clone();
    let on_now = {
        let (rel, dir) = (rel.clone(), dir.clone());
        move |_| {
            choose(&dir, &rel, true);
            *LOBBY.write() = Lobby::Hidden;
            let v = rel.version.clone();
            dioxus_core::spawn_forever(async move { say(format!("アップデートしました(バージョン {v})")).await });
        }
    };
    let on_later = {
        let (rel, dir) = (rel.clone(), dir.clone());
        move |_| {
            choose(&dir, &rel, false);
            *LOBBY.write() = Lobby::Hidden;
        }
    };
    rsx! {
        div { class: "lobby",
            div { class: "lobby__body",
                h1 { class: "lobby__title", "アップデートがあります" }
                p { class: "lobby__version", "バージョン {version}" }
                if !rel.urgent_fixed.is_empty() {
                    section { class: "lobby__section lobby__section--urgent",
                        h2 { "先に直したところ" }
                        p { class: "lobby__lead", "安全のため、アップデートを待たずに、もう直してあります。" }
                        ul { for item in rel.urgent_fixed.iter() { li { "{item}" } } }
                    }
                }
                if !rel.new.is_empty() {
                    section { class: "lobby__section",
                        h2 { "新しくなったところ" }
                        ul { for item in rel.new.iter() { li { "{item}" } } }
                    }
                }
                if !rel.fixed.is_empty() {
                    section { class: "lobby__section",
                        h2 { "直したところ" }
                        ul { for item in rel.fixed.iter() { li { "{item}" } } }
                    }
                }
            }
            div { class: "lobby__actions",
                button { class: "lobby__button lobby__button--primary", onclick: on_now, "いまアップデートする" }
                button { class: "lobby__button", onclick: on_later, "ダウンロードだけして、つぎに開くときアップデートする" }
            }
        }
    }
}
