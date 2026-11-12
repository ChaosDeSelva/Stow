# ADR-0003: Localhost HTTP endpoint for the Chrome extension

- Status: Accepted
- Date: 2026-11-08

## Context
The hotkey captures only the front tab, and only through AppleScript permission. Users also want a toolbar button and a "Stow this link" context menu. The extension has to deliver a URL to the running app and list buckets for its options page.

## Decision
- The app runs an `axum` server bound to **127.0.0.1 only**. It first tries the previously saved port, then 47823, then any free port, so an already-configured extension keeps working across restarts.
- Routes: `GET /health` (open), plus `GET /buckets` and `POST /capture`, which require `Authorization: Bearer <token>`. The token is generated once per install and can be rotated from Settings, which invalidates the old one immediately.
- Pairing is a single `port:token` code shown in Settings, or `POST /pair {name}` from the extension, which the user approves or denies in the app (the request waits up to 60 s and returns `{token, port}` on approval).
- CORS allows only `chrome-extension://` origins; Settings → Chrome extension → **Extension ID (pins CORS)** narrows it to one id (the `extension_id` setting). Once the extension ships on the Web Store, its fixed ID is the one to pin.
- Captures go through the same core path as the hotkey: a synchronous insert, then background jobs.

## Consequences
- Setup is one paste (or one click on "Allow") and works with an unpacked extension. There's no extra installer step.
- Any local process can reach the port, so the token is the only gate. The extension keeps it in `chrome.storage.local` (not `sync`), and the app treats it as a secret.
- If both preferred ports are taken, the port changes and the extension must be re-paired.
- DNS rebinding is a concern for any localhost server: the token requirement and the `chrome-extension://` origin check defend against it, and (since the v0.1 hardening) the endpoint also rejects any request whose `Host` header is not `127.0.0.1:<port>` or `localhost:<port>`, caps bodies at 16 KiB, and compares the token in constant time.

## Alternatives considered
- **Chrome Native Messaging.** No open port, and Chrome authenticates the extension ID. But it requires installing a host manifest into Chrome's profile directory and a fixed extension ID, which is awkward for unpacked installs. This is the preferred target once the extension ships on the Chrome Web Store.
- **A custom URL scheme (`stow://add?url=`).** No response channel (can't list buckets or confirm success), and it brings the app to the front.
- **AppleScript only.** No toolbar button or link capture.
