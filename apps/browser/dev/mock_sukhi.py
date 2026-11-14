#!/usr/bin/env python3
"""Mock of the three /_app/ login endpoints, for building the shell before the real server exists.
Usage: mock_sukhi.py [port]   (default 8078). Not for production: no real login, no persistence."""
import base64, hashlib, json, secrets, sys, time
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import urlparse, parse_qs

CODES = {}   # code -> (challenge, expires_at)
SCHEME = "social.sukhi.shell://callback"
RELEASE = __import__("os").path.join(__import__("os").path.dirname(__file__), "release.json")

def release_bytes(version): return (version.encode() * 40000)[:200_000]  # ~200 KB, deterministic

def b64url(b): return base64.urlsafe_b64encode(b).rstrip(b"=").decode()

class H(BaseHTTPRequestHandler):
    def _send(self, status, body=b"", ctype="text/html; charset=utf-8", headers=()):
        self.send_response(status); self.send_header("Content-Type", ctype)
        for k, v in headers: self.send_header(k, v)
        self.end_headers(); self.wfile.write(body)

    def do_GET(self):
        u = urlparse(self.path); q = parse_qs(u.query)
        if u.path == "/_app/login":
            ch, st = q.get("challenge", [""])[0], q.get("state", [""])[0]
            if not ch or not st: return self._send(400, b"challenge and state are required")
            confirm = f"/_app/login/confirm?challenge={ch}&state={st}"
            body = ("<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>"
                    "<body style='font-size:28px;padding:24px'><p>(模擬) この端末の殻に、nyanrus でログインします。</p>"
                    f"<p><a style='font-size:36px' href='{confirm}'>ログインする</a></p></body>").encode()
            return self._send(200, body)
        if u.path == "/_app/login/confirm":  # the real one is a POST; GET keeps the mock tappable
            ch, st = q.get("challenge", [""])[0], q.get("state", [""])[0]
            code = secrets.token_urlsafe(32); CODES[code] = (ch, time.time() + 60)
            return self._send(302, headers=[("Location", f"{SCHEME}?code={code}&state={st}")])
        if u.path == "/_app/manifest.json":  # edit release.json while this runs to change the release
            rel = json.load(open(RELEASE))
            rel["bytes_url"] = f"/_app/releases/{rel['version']}.bin"
            rel["sha256"] = hashlib.sha256(release_bytes(rel["version"])).hexdigest()
            return self._send(200, json.dumps(rel, ensure_ascii=False).encode(), "application/json")
        if u.path.startswith("/_app/releases/") and u.path.endswith(".bin"):
            return self._send(200, release_bytes(u.path.rsplit("/", 1)[1][:-4]), "application/octet-stream")
        self._send(404)

    def do_POST(self):
        if urlparse(self.path).path != "/_app/token": return self._send(404)
        try: j = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"{}")
        except ValueError: j = {}
        code, verifier = j.get("code", ""), j.get("verifier", "")
        row = CODES.pop(code, None)  # single use, even when the verifier is wrong
        ok = row and row[1] >= time.time() and secrets.compare_digest(row[0], b64url(hashlib.sha256(verifier.encode()).digest()))
        if not ok: return self._send(400, b'{"error":"invalid_grant"}', "application/json")
        self._send(200, json.dumps({"sid": secrets.token_urlsafe(24)}).encode(), "application/json")

if __name__ == "__main__":
    HTTPServer(("0.0.0.0", int(sys.argv[1]) if len(sys.argv) > 1 else 8078), H).serve_forever()
