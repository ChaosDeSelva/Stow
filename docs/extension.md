# Chrome extension

The extension adds a **Stow this page** toolbar button and right-click items for **Stow this page** and **Stow this link**. It sends the URL to the Stow app running on your Mac and never talks to anything except `http://127.0.0.1:<port>`.

You don't need the extension for the main workflow: ⌃⌘S works without it. It's handy for stowing links you haven't opened, or when you'd rather click than press keys.

## Install

1. Make sure Stow is running (it lives in the menu bar).
2. In Chrome, open `chrome://extensions` and turn on **Developer mode** (top right).
3. Click **Load unpacked** and choose the extension folder. Stow ships one inside the app: Settings → **Chrome extension** → **Install** lists these steps, and **Show extension folder** reveals it in Finder (`Stow.app/Contents/Resources/extension`). You can also use the `extension/` folder of a checkout, or unzip `stow-extension_<version>.zip` from the [release](https://github.com/ChaosDeSelva/Stow/releases/latest).
4. Pin Stow from the puzzle-piece menu so the button is visible.

A Chrome Web Store listing is on the roadmap.

## Pair it with the app

Open the extension's options (right-click the Stow toolbar button → **Options**, or click the button while unpaired and the options open by themselves). Either way takes a few seconds:

**Pair automatically.** Click **Pair automatically**. The extension asks Stow (on the saved port, then 47823); Stow brings its window forward and asks **Allow the Chrome extension to save tabs to Stow?**, naming the extension, with **Deny** and **Allow**. Click **Allow** and the extension receives its own code. The request expires after 60 seconds, and only one request can be waiting at a time.

**Or paste the pairing code.** In Stow, open Settings → **Chrome extension** and click **Copy pairing code** (Get set up has the same button under **Add the Chrome button**). The code looks like `47823:3f9a…` (port, colon, a 32-character token). Paste it into **Pairing code** in the extension options and click **Save**. The status line confirms the connection and how many buckets it found.

Then, optionally, choose **Save into** (a bucket; otherwise links go to the Inbox) and **Close the tab after saving**.

## Using it

| Action | Result |
|---|---|
| Click the toolbar button | Stows the current page (and closes the tab if you enabled that) |
| Right-click a page → **Stow this page** | Same as the button |
| Right-click a link → **Stow this link** | Stows the link's target. The current tab is never closed. |

The button badge shows the result for a few seconds: green ✓ means saved (the tooltip says "Stowed" or "Already in Stow"), red ✕ means it failed, and amber ⚙ means the extension isn't paired yet (the options page opens). Hover the button for the reason.

If the bucket you chose in **Save into** is later deleted in Stow, links fall back to the Inbox.

## Security

- The extension talks only to `http://127.0.0.1:<port>`; the token is sent as a bearer header. It's stored in `chrome.storage.local`, so it never syncs to your Google account. (Settings saved by an older build in `chrome.storage.sync` are moved to local storage once and removed from sync.)
- Permissions: `activeTab`, `contextMenus`, `storage`, and the host permission `http://127.0.0.1:*/*`. Nothing else.
- Treat the pairing code like a password. Anyone with it and access to your Mac could add links to Stow. **Reset pairing code** (Settings → Chrome extension, or Settings → Data → Danger zone) invalidates it immediately; pair again afterwards.
- Only `http`/`https` pages can be stowed. `chrome://`, `file://`, and the Web Store are skipped.
- Stow accepts requests only from `chrome-extension://` origins. If you want to lock it to this one extension, copy its ID from `chrome://extensions` into Settings → Chrome extension → **Advanced** → **Extension ID (pins CORS)**.

## Troubleshooting

- **✕ on every click.** Hover the button: "Stow isn't running" means open the app; "Pairing code changed" means the code was reset in Stow, so paste the new one or click **Pair automatically**. If the port changed (this happens only when 47823 was taken by another app and Stow had to pick a random one), pairing again fixes it.
- **"Couldn't reach Stow" in the options.** Make sure Stow is running and that no firewall tool blocks loopback connections. Clicking **Save** again retries.
- **"That doesn't look like a pairing code."** The code must be `<port>:<32 hex characters>`; copy it again from Settings → Chrome extension.
- **"Stow declined the pairing request."** Deny was clicked or the 60 s expired. Try again and click **Allow** in Stow.
