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
