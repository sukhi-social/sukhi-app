//! sukhi shell: login through the system browser (see dev/CONTRACT.md).
//!
//! The shell makes a PKCE verifier + state, asks Kotlin to open `{origin}/_app/login` in the
//! system browser, and later receives `social.sukhi.shell://callback?code&state`. It trades
//! code + verifier for a session id (`sukhi_sid`), keeps it in a private file, and puts it in the
//! page cookie jar. Gateway tokens never reach the shell.

use std::path::{Path, PathBuf};
use std::time::Duration;

use dioxus_native::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// What the login page shows.
#[derive(Clone, PartialEq, Debug)]
pub enum LoginState {
    Idle,
    /// The browser was opened; waiting for the person to come back.
    Waiting,
    Failed(String),
}

pub static LOGIN: GlobalSignal<LoginState> = Signal::global(|| LoginState::Idle);

#[derive(Serialize, Deserialize)]
struct Pending {
    verifier: String,
    state: String,
}

fn b64url(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 2);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        if c.len() > 1 {
            out.push(T[(n >> 6) as usize & 63] as char);
        }
        if c.len() > 2 {
            out.push(T[n as usize & 63] as char);
        }
    }
    out
}

fn random_token() -> Option<String> {
    use std::io::Read;
    let mut buf = [0u8; 32];
    std::fs::File::open("/dev/urandom").ok()?.read_exact(&mut buf).ok()?;
    Some(b64url(&buf))
}

fn file(dir: &Path, name: &str) -> PathBuf {
    dir.join(name)
}

/// The saved session id, if this device is logged in.
pub fn saved_sid(dir: &Path) -> Option<String> {
    std::fs::read_to_string(file(dir, "shell-sid.txt"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Put the saved session id into the cookie jar (at launch, and right after login).
pub fn inject_cookie(origin: &str, sid: &str) {
    #[cfg(feature = "cookies")]
    if let Ok(url) = blitz_traits::net::Url::parse(origin) {
        let secure = if url.scheme() == "https" { "; Secure" } else { "" };
        blitz_net::cookie_jar().add_cookie_str(
            &format!("sukhi_sid={sid}; Path=/; HttpOnly; SameSite=Lax{secure}"),
            &url,
        );
    }
    #[cfg(not(feature = "cookies"))]
    let _ = (origin, sid);
}

/// Open the system browser on the login page. Kotlin watches `shell-open.txt`.
pub fn start(dir: &Path, origin: &str) {
    let (Some(verifier), Some(state)) = (random_token(), random_token()) else {
        *LOGIN.write() = LoginState::Failed("乱数をつくれませんでした".into());
        return;
    };
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    let pending = Pending { verifier, state: state.clone() };
    if let Ok(s) = serde_json::to_string(&pending) {
        let _ = std::fs::write(file(dir, "login-pending.json"), s);
    }
    let url = format!("{origin}/_app/login?challenge={challenge}&state={state}");
    let _ = std::fs::write(file(dir, "shell-open.txt"), url);
    *LOGIN.write() = LoginState::Waiting;
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    q.split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.to_owned())
}

/// Handle `social.sukhi.shell://callback?code=…&state=…`. Returns true when logged in.
pub async fn complete(dir: &Path, origin: &str, callback: &str) -> bool {
    let fail = |msg: &str| {
        *LOGIN.write() = LoginState::Failed(msg.to_owned());
        false
    };
    let pending: Option<Pending> = std::fs::read_to_string(file(dir, "login-pending.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let Some(pending) = pending else {
        return fail("ログインを始めた記録がありません。もう一度ためしてください");
    };
    // One try per login: a second callback with the same state must not work.
    let _ = std::fs::remove_file(file(dir, "login-pending.json"));
    if query_param(callback, "state").as_deref() != Some(pending.state.as_str()) {
        return fail("ログインの確認が合いませんでした。もう一度ためしてください");
    }
    let Some(code) = query_param(callback, "code") else {
        return fail("ログインの確認がとどきませんでした");
    };
    let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(15)).build() else {
        return fail("通信を始められませんでした");
    };
    let body = serde_json::json!({ "code": code, "verifier": pending.verifier });
    let resp = client
        .post(format!("{origin}/_app/token"))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await;
    let Ok(resp) = resp else {
        return fail("つながりませんでした。電波のよいところで、もう一度ためしてください");
    };
    if !resp.status().is_success() {
        return fail("ログインの時間が切れました。もう一度ためしてください");
    }
    let sid = resp
        .text()
        .await
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|j| j.get("sid").and_then(|s| s.as_str()).map(str::to_owned));
    let Some(sid) = sid else {
        return fail("返事を読めませんでした");
    };
    // The sid lives only in the app's private files (a Keystore wrapper is a later step).
    let _ = std::fs::write(file(dir, "shell-sid.txt"), &sid);
    inject_cookie(origin, &sid);
    *LOGIN.write() = LoginState::Idle;
    true
}

pub fn logout(dir: &Path) {
    let _ = std::fs::remove_file(file(dir, "shell-sid.txt"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b64url_matches_known() {
        assert_eq!(b64url(b""), "");
        assert_eq!(b64url(b"f"), "Zg");
        assert_eq!(b64url(b"fo"), "Zm8");
        assert_eq!(b64url(b"foo"), "Zm9v");
        assert_eq!(b64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn reads_query() {
        let u = "social.sukhi.shell://callback?code=abc&state=xyz";
        assert_eq!(query_param(u, "code").as_deref(), Some("abc"));
        assert_eq!(query_param(u, "state").as_deref(), Some("xyz"));
        assert_eq!(query_param(u, "nope"), None);
    }
}
