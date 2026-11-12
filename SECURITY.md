# Security Policy

## Supported versions

Stow is pre-1.0. The latest minor release receives security fixes; older releases are not supported. The Chrome extension is supported at the version published with the latest app release.

| Version | Supported |
|---|---|
| Latest minor (e.g. 0.x.y, newest x) | Yes |
| Older | No |

## Reporting a vulnerability

**Please don't open a public issue.**

- Preferred: [GitHub private vulnerability reporting](https://github.com/ChaosDeSelva/Stow/security/advisories/new).
- Alternative: email SECURITY_EMAIL.

Please include:

- The Stow version (shown at the bottom of Settings → Data, or `stw --version`) and your macOS version.
- Steps to reproduce or a proof of concept (for example, a hostile HTML page that Stow processes).
- The impact you see (what an attacker gains, and from where).
- Whether the issue is already public anywhere.

## What to expect

- Acknowledgement within **3 business days**.
- An assessment and a severity rating (CVSS) within **10 business days**.
- Fix targets: Critical and High within **30 days**, Medium within **60 days**, Low in the next regular release.
- Coordinated disclosure through a GitHub Security Advisory (with a CVE where applicable) when the fix ships, or at 90 days from the report, whichever comes first.
- Credit in the advisory and release notes unless you prefer to stay anonymous.

## Scope

In scope: the macOS app, `stow-core`, `stow-cli`, the Chrome extension, the localhost capture endpoint, and the build/release pipeline. We are especially interested in:

- Hostile web pages processed by Stow: HTML parsing and readability extraction, SSRF via fetched URLs, DNS answers or redirects, and rendering of snapshots, notes, or AI output in the webview.
- The localhost endpoint: authentication bypass, DNS rebinding, CORS mistakes, or pairing abuse.
- Prompt injection through saved pages or transcripts that leads AI actions to exfiltrate data.
- IPC or asset-protocol scope escapes between the webview and the Rust side.
- Keychain handling of API keys.

Out of scope:

- An attacker who already runs code as the same macOS user (they can read the data folder directly).
- Vulnerabilities in third-party AI providers.
- Social engineering of users or maintainers.
- Hardening gaps without a demonstrated impact (for example, a missing header with no exploit path). We still welcome those as ordinary issues.

## Safe harbor

Good-faith security research that follows this policy, uses only your own installation and data, and avoids privacy violations or data destruction will not result in legal action from the project. If in doubt, ask first through the private channel.

## Security model summary

- **Local-first.** No account, no telemetry, no server run by the project. Data lives in `~/Library/Application Support/Stow` (mode `0700`: `stow.db` plus snapshot, attachment, audio, asset, and model files).
- **API keys** are stored only in the macOS Keychain (service `io.github.chaosdeselva.stow`, accounts `anthropic` and `openai`), never in SQLite, settings, logs, job payloads, exports, or frontend events. The UI is write-only: no command returns a key.
- **Network egress**, in full: page fetches for URLs you save (the page, its favicon, its preview image, and `www.youtube.com/oembed` for YouTube links); remote images inside saved pages only when you enable **Load images** for that item; the Whisper model download from `huggingface.co` (revision-pinned, SHA-256 verified); and `api.anthropic.com` or `api.openai.com`, only when you click an AI action.
- **Fetching is SSRF-guarded.** Hostnames are resolved with the system resolver and any non-public address is dropped (loopback, RFC 1918, link-local including `169.254.169.254`, CGNAT, multicast, reserved, IPv6 ULA/link-local, and IPv4-mapped/NAT64/6to4 forms of those); the peer address of every connection is checked after connecting, so literal-IP URLs are caught too; redirects are re-checked per hop, limited to `http`/`https`, and capped at 10; system proxies are ignored. Pages are capped at 10 MiB, images at 5 MiB with an image content-type allowlist, and HTML nesting at 512 levels. Links to local addresses are saved but marked "local address not fetched" unless the user turns on **Fetch pages on private or local addresses** (Settings → Data → Advanced).
- **Capture endpoint.** The app binds `127.0.0.1` only, on the saved port, then 47823, then a random free port. `GET /buckets` and `POST /capture` require a per-install 32-hex-character bearer token (`Authorization: Bearer <token>`), compared in constant time and re-read from settings on every request, so **Reset pairing code** invalidates the old one immediately. Requests whose `Host` header is not `127.0.0.1:<port>` or `localhost:<port>` get 400; bodies over 16 KiB get 413; URLs over 8 KiB or titles over 1 KiB get 400. CORS allows only `chrome-extension://` origins, or exactly one extension id when it is pinned in Settings → Chrome extension. `POST /pair` needs no token but returns one only after the user clicks **Allow** in the app within 60 s; one request may wait at a time (429 otherwise).
- **Rendering.** Markdown is sanitized with DOMPurify; images, forms, iframes, media, SVG and MathML are forbidden in notes and AI output. The webview has a CSP (`default-src 'self'`, no remote scripts), navigation away from the app is blocked, `withGlobalTauri` is off, and the asset and opener scopes are narrowed to the data folder's `assets/`, `audio/`, and `attachments/` subfolders (plus the data folder itself for "Open in Finder").
- **Input validation.** All ids are validated as ULIDs and all enum-like settings against allowlists at the command boundary; export and backup destinations must be absolute paths.
- **AI prompts** wrap page text and transcripts in `<untrusted_page>` / `<untrusted_transcript>` blocks (truncated to 60,000 characters, closing tags neutralized) and tell the model to ignore instructions inside them.
- **Chrome integration** goes through AppleScript (Automation permission). Chrome is checked with `pgrep` before any script runs so it is never launched, URLs are passed as arguments and never interpolated into scripts, and a tab is closed only if its URL still matches the one that was captured.
- **Hardened runtime** entitlements are limited to `com.apple.security.device.audio-input` and `com.apple.security.automation.apple-events`; `LSFileQuarantineEnabled` is set.

The maintainers' hardening checklist is in [docs/security/hardening-checklist.md](docs/security/hardening-checklist.md).
