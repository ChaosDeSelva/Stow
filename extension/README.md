# Stow Chrome extension

A Manifest V3 extension that sends the current tab (or a right-clicked link) to the Stow app on this Mac over `http://127.0.0.1:<port>`, authenticated with a per-install bearer token. Nothing else is contacted.

User setup is in [docs/extension.md](../docs/extension.md). The app bundles this folder at `Stow.app/Contents/Resources/extension` so users can load it unpacked.

## Files

| File | What |
|---|---|
| `manifest.json` | MV3 manifest. Permissions: `activeTab`, `contextMenus`, `storage`; host permission `http://127.0.0.1:*/*`. |
| `background.js` | Service worker: toolbar click, context menus (**Stow this page**, **Stow this link**), `POST /capture`, badge feedback, optional tab close. |
| `options.html`, `options.js` | Options page: **Pairing code** (`port:token`) with **Save**, **Pair automatically** (`GET /health` then `POST /pair`, approved in the app), **Save into** bucket (from `GET /buckets`), **Close the tab after saving**. |
| `lib.js` | Pure helpers (pairing-code parsing, port/token validation, endpoint URL building, failure copy). No Chrome APIs, so they're unit-testable. |
| `lib.test.mjs` | `node --test` tests for `lib.js`. Not shipped in the zip. |
| `icons/` | 16/32/128 px icons. |

## Develop

```bash
node --check extension/background.js extension/options.js extension/lib.js
node --test 'extension/*.test.mjs'
scripts/package-extension.sh out      # out/stow-extension_<version>.zip for releases / the Web Store
```

Load it unpacked from `chrome://extensions` (Developer mode → Load unpacked → this folder). After editing `background.js`, click the reload icon on the extension card.

## Rules

- Settings live in `chrome.storage.local`, never `chrome.storage.sync`: the token must not leave the machine. `settings()` migrates a legacy `sync` record once.
- Build URLs with `endpoint(port, path)` from `lib.js`; never interpolate raw strings into a URL. Tokens are 32 lowercase hex characters; ports 1–65535.
- Only `http`/`https` tab URLs are sent. A link capture never closes the tab it was clicked in.
- The version in `manifest.json` is kept in step with the app by `scripts/sync-version.sh` (pre-release suffixes are dropped; the Web Store requires numeric versions).
