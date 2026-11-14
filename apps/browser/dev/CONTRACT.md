# sukhi shell ⇔ sukhi.social: login contract

The shell is a *special OAuth client*: it never sees gateway tokens. It starts a login in the
system browser (which reuses web-next's /login and /check pages), gets back a one-time `code`,
and trades it (plus its PKCE `verifier`) for a web-next session id (`sukhi_sid`).

Shell → browser (opened with ACTION_VIEW):
  GET {origin}/_app/login?challenge=<b64url(sha256(verifier))>&state=<random>

Browser → shell (302 after the person confirms; later an https App Link):
  social.sukhi.shell://callback?code=<one-time>&state=<same state>

Shell → server (native HTTP, no cookies):
  POST {origin}/_app/token   {"code": "...", "verifier": "..."}
  200 {"sid": "<sukhi_sid>"}          400 {"error": "invalid_grant"}

Rules: code is single-use, expires in 60 s, bound to its challenge. state must match what the
shell sent. The shell puts `sid` in the page cookie jar as `sukhi_sid` (HttpOnly) and in Keystore.

## As built (2026-11-15)
- Real server: web-next `server/routes/app.ts` (sukhi-fedi branch feat/web-next-app-endpoints). `confirm` is a POST
  with a one-time nonce (the mock keeps GET so it stays tappable). The token is *moved* to a new session.
- Shell: `apps/browser/src/login.rs`. Opens the browser through `files/shell-open.txt` (Kotlin watches it), keeps
  verifier + state in `files/login-pending.json` (deleted on first callback, so one try per login), keeps `sid` in
  `files/shell-sid.txt` (app-private; Keystore is later) and puts it in the shared jar `blitz_net::cookie_jar()`.
- Launch: no saved sid → `about:shell-login`; saved sid → cookie injected before the first load.
