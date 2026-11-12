# ADR-0007: Structured command errors with stable codes

- Status: Accepted
- Date: 2026-11-08

## Context
Tauri commands originally rejected with a plain `String` (`StowError::to_string()`). The frontend had to pattern-match message text to decide what to show ("permission_denied" vs "chrome_not_running" vs an HTTP failure), and any wording change in Rust silently broke that mapping. Messages also mixed user-facing copy with debugging detail, so they were either too terse to debug or too technical to show.

## Decision
- Every command rejects with `CmdError { code: string, message: string }`, serialized as JSON. `code` is a stable, machine-readable identifier; `message` is detail for a "Details" disclosure and logs.
- The code set is enumerated in the [IPC contract](../ipc-contract.md): `not_found`, `invalid_url`, `invalid_input`, `db`, `io`, `http`, `not_html`, `too_large`, `keychain`, `audio`, `transcribe`, `provider`, `chrome_not_running`, `permission_denied`, `no_tab`, `script_error`, `hotkey`, `busy` (the same background job is already queued or running), `schema` (the database was written by a newer Stow), `internal`. Adding a code is a contract change and goes in the same PR as the frontend copy for it.
- `stow_core::StowError` variants map to codes in one place (`src-tauri/src/error.rs`). Chrome errors map from `osascript` exit text; AI provider errors use `provider` with the provider name and HTTP status at the start of `message`.
- The frontend has a single `friendlyError(e)` that maps `code` to user copy and falls back to string parsing only for legacy strings. Components never inspect `message`.
- Messages never contain secrets (API keys, tokens) or full page bodies.

## Consequences
- User-facing copy lives in the frontend and can be edited without touching Rust; Rust messages can become more precise without breaking the UI.
- Tests assert on `code`, which is stable, instead of on sentences.
- The CLI keeps printing `StowError`'s `Display` text; only the Tauri boundary is structured.
- Every new error path has to pick a code deliberately. `internal` is the catch-all and should be rare; a growing count of `internal` in logs is a signal to add a code.

## Alternatives considered
- **Keep plain strings with a `code: ` prefix convention.** Cheap, but still string parsing, and nothing enforces the prefix.
- **Serialize the full `StowError` enum.** Leaks internal structure (variant names, `rusqlite` details) into the UI, and any refactor of the enum becomes a contract change.
- **Numeric codes.** Compact but opaque in logs and bug reports; snake_case strings are self-describing and cost nothing.
