# IPC contract

The Tauri command and event contract between `apps/stow-app/src` (TypeScript) and `apps/stow-app/src-tauri` (Rust) lives at:

**[`docs/superpowers/plans/tauri-contract.md`](../superpowers/plans/tauri-contract.md)**

That file is the single source of truth. It defines command names, JS argument shapes (camelCase), returned structs (snake_case, mirroring `models.rs`), enum spellings, events, structured error codes, and the rendering/security rules. Update it in the same PR as any command or event change (see [CONTRIBUTING.md](../../CONTRIBUTING.md#adding-a-tauri-command-end-to-end)).
