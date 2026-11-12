# Architecture

Stow is a Cargo workspace with three members and a browser extension:

- **stow-core** (`crates/stow-core`) owns all data and logic. It takes a data directory, opens SQLite, applies migrations, and exposes plain repo functions plus an in-process job queue (metadata, snapshot, transcription, AI). It has no UI dependency, so the app, the CLI, and tests all drive the same code. Cargo features `audio`, `transcribe`, `ai`, `keychain` are on by default; `metal` is enabled by the app.
- **stow-cli** (`crates/stow-cli`) is a thin clap front end over core, built with core's default features off (no audio, Whisper, or AI). The binary is `stw`.
- **stow-app** (`apps/stow-app`) is the Tauri 2 shell. `src-tauri/` owns everything macOS-specific: windows, tray, global hotkey, AppleScript to Chrome, notifications, microphone, permission checks, the reminder scheduler, and the localhost endpoint for the extension. `src/` is the Svelte 5 frontend, which talks to Rust only through the commands and events in the [IPC contract](ipc-contract.md).
- **extension/** is a Chrome MV3 extension that POSTs the current tab to the app over `127.0.0.1`. The app bundles it under `Resources/extension`.

Invariants worth preserving:

1. Capture is synchronous to SQLite. Network work is always a background job, and every job's HTTP client carries the SSRF guards ([jobs-and-fetch.md](jobs-and-fetch.md)).
2. Nothing user-authored leaves the machine without an explicit user action.
3. API keys live only in the Keychain: never in SQLite, logs, exports, or events.
4. Shipped migrations are immutable; a newer database is refused, not guessed at ([data-model.md](data-model.md)).
5. Command errors carry a stable `code`; the frontend never parses messages.
6. Every id and enum-like value is validated at the command boundary before it reaches the database or a file path.

## Documents

| Document | What |
|---|---|
| [data-model.md](data-model.md) | File layout, tables, triggers, search index, migrations and `user_version`, export and backup formats |
| [jobs-and-fetch.md](jobs-and-fetch.md) | Job queue semantics (dedupe, limits, resume), what each job does, SSRF and size rules, local-URL policy |
| [ipc-contract.md](ipc-contract.md) | Where the Tauri command/event contract lives |
| [design-spec.md](design-spec.md) | The original design spec (historical) |
| [implementation-plan.md](implementation-plan.md) | The original task-by-task plan (historical) |
| [../testing.md](../testing.md) | Test matrix and backlog |
| [../security/hardening-checklist.md](../security/hardening-checklist.md) | Release hardening checklist |

## Decision records

| ADR | Decision |
|---|---|
| [0000](adr/0000-template.md) | Template |
| [0001](adr/0001-sqlite-and-plain-files.md) | SQLite as source of truth, plain files for blobs |
| [0002](adr/0002-tauri-and-svelte.md) | Tauri 2 + Svelte 5 |
| [0003](adr/0003-localhost-extension-endpoint.md) | Localhost HTTP endpoint for the Chrome extension |
| [0004](adr/0004-keychain-for-api-keys.md) | macOS Keychain for API keys |
| [0005](adr/0005-on-device-whisper.md) | On-device transcription with whisper.cpp |
| [0006](adr/0006-polymorphic-parents-with-triggers.md) | Polymorphic parent columns with delete-cascade triggers, not foreign keys |
| [0007](adr/0007-structured-command-errors.md) | Structured command errors with stable codes |

New decisions get the next number and follow the template. A superseded ADR keeps its file and gets `Status: Superseded by ADR-XXXX`.
