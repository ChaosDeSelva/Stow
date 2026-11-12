# ADR-0002: Tauri 2 with a Svelte 5 frontend

- Status: Accepted
- Date: 2026-11-08

## Context
Stow needs a menu-bar-resident macOS app with a global hotkey, a floating capture panel, native notifications, microphone access, and AppleScript to Chrome. It also needs a Rust core shared with a CLI. It should be small, fast to start, and approachable for contributors.

## Decision
- Tauri 2 for the shell. Its plugins provide global-shortcut, notification, clipboard, dialog, opener, and autostart. Everything else lives in our Rust.
- Svelte 5 (runes) on plain Vite with TypeScript and hash routing (`#/capture` renders the capture window). We don't use SvelteKit, because there's no server and no file routing to benefit from.
- The UI reaches Rust only through commands and events defined in the [IPC contract](../ipc-contract.md), wrapped in `src/lib/api.ts`.
- No component library: a small stylesheet with CSS custom properties. Light is the default theme; dark tokens live under `[data-theme="dark"]`.

## Consequences
- The Rust core is reused unchanged by the CLI and tests. The bundle is a few MB plus whisper.cpp, compared with ~100 MB for Electron.
- The UI runs in WKWebView, so we're bound to Safari's engine and its quirks, and there is no WebDriver for it on macOS (see [docs/testing.md](../../testing.md)).
- macOS integration that Tauri doesn't cover (Chrome via `osascript`, permission status via TCC APIs) is ours to maintain.
- The contract doc must be kept in sync by hand. Its conventions (snake_case commands, camelCase args, snake_case returned fields) are easy to get wrong, and PR review checks them.

## Alternatives considered
- **Swift/SwiftUI.** The best native feel, but no shared core with a cross-platform CLI and a smaller contributor pool.
- **Electron.** A mature ecosystem, but heavy for a menu-bar utility, and the core would be JS or need N-API bindings.
- **React or SvelteKit.** More ceremony than a few panes need. Svelte 5 runes keep stores small.
