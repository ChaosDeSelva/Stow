# Getting help

- **Questions, ideas, "how do I…"**: use [GitHub Discussions](https://github.com/ChaosDeSelva/Stow/discussions).
- **Bugs**: open an [issue](https://github.com/ChaosDeSelva/Stow/issues/new/choose) using the bug template.
- **Security problems**: follow [SECURITY.md](SECURITY.md). Never post them publicly.

Stow is maintained in spare time, so there is no guaranteed response time. Clear reports get fixed first.

## Before you file a bug

1. Update to the [latest release](https://github.com/ChaosDeSelva/Stow/releases/latest).
2. Check the [user guide](docs/user-guide.md) and existing issues.
3. Collect diagnostics:
   - Stow version (bottom of Settings → Data, or `stw --version`), macOS version, Apple Silicon or Intel.
   - Logs: quit Stow from the menu bar, then run
     `RUST_LOG=debug /Applications/Stow.app/Contents/MacOS/stow-app 2>&1 | tee ~/Desktop/stow.log`
     and reproduce the problem.
   - For CLI problems: `stw --version` and the full command output.

Look through logs before attaching them. They can contain URLs and titles from your library. API keys are never logged.

## Common fixes

| Symptom | Fix |
|---|---|
| ⌃⌘S opens a capture panel that asks you to paste a URL | Allow Stow under System Settings → Privacy & Security → Automation → Google Chrome. Settings → Permissions has **Open Privacy Settings** for each pane. |
| ⌃⌘S does nothing | Another app owns the shortcut. Settings → Capture → **Quick capture shortcut** shows whether registration succeeded; record a different combination. |
| No reminder notifications | System Settings → Notifications → Stow → Allow. Make sure Stow is still running in the menu bar (Settings → General → "When I close the window" → **Keep Stow in the menu bar**). |
| Recording fails | Allow Stow under Privacy & Security → Microphone. |
| Transcription stuck on "downloading model" | Check your connection and disk space; delete `models/ggml-*.bin.part` in the data folder and retry. |
| A readable copy says "local address not fetched" | Links to `localhost` or private addresses are saved but not fetched by default. Turn on Settings → Data → Advanced → **Fetch pages on private or local addresses** and restart Stow. |
| Extension shows ✕ | Hover the button for the reason. Open the extension's options and click **Save** (re-saving the code reconnects and lists your buckets) or **Pair automatically**. If the pairing code was reset in Stow, paste the new one (see [docs/extension.md](docs/extension.md)). |
| "Stow couldn't open its database" / schema is newer | The data folder was last written by a newer Stow. Update Stow, or point the old version at a different `STOW_DATA_DIR`. |
