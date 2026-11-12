# ADR-0004: Store AI API keys in the macOS Keychain

- Status: Accepted
- Date: 2026-11-08

## Context
AI actions are bring-your-own-key (Anthropic or OpenAI). Keys are billable credentials. The data folder is meant to be copied, backed up, exported, and inspected, so it's the wrong place for secrets.

## Decision
- Keys are stored with the `keyring` crate in the login Keychain under service `io.github.chaosdeselva.stow` (the bundle identifier), with accounts `anthropic` and `openai`. v0.1 pre-release builds used service `stow`; `keys::get` migrates such an entry to the new service on first read and removes the old one.
- They are never written to SQLite, settings, logs, job payloads, or frontend events.
- The UI is write-only: `set_api_key`, `has_api_key`, and `delete_api_key`. No command returns a key.
- The provider and model are ordinary settings. The key is read from the Keychain at call time.

## Consequences
- Copying or sharing the data folder never leaks a key. Restoring a backup on a new Mac means re-entering keys.
- Keychain access is tied to the app's code signature. Dev builds, unsigned builds, and a change of signing identity can trigger "Stow wants to use your confidential information" prompts.
- Tests must not touch the real Keychain. Core accepts a key directly so tests can bypass it.

## Alternatives considered
- **Settings table or a config file.** Simplest, but leaks into backups, bug reports, and screenshots.
- **Environment variables.** Fine for the CLI, but unusable for a GUI app launched from Finder.
- **Encrypting the key ourselves.** We would still need to store an encryption key somewhere, which leads back to the Keychain.
