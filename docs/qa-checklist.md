# Stow QA checklist (v0.1 release candidate)

Build: `npm run tauri build` in `apps/stow-app` puts `Stow.app` under `target/release/bundle/macos/` at the workspace root (`target/aarch64-apple-darwin/release/bundle/macos/` with `--target aarch64-apple-darwin`); `npm run tauri dev` runs it unbundled. The CLI is `cargo build -p stow-cli --release` → `target/release/stw`.
Use a fresh data dir for a clean first run. `open` does not pass environment variables through, so run the binary directly: `STOW_DATA_DIR=/tmp/stow-qa <bundle>/Stow.app/Contents/MacOS/stow-app`.

Mark each row ✅ / ❌ with a note. Anything ❌ is a bug. The companion walkthrough for users is [user-guide.md](user-guide.md).

## 1. First launch
- [ ] App opens on the light theme with **Get set up** at the top of the sidebar (badge `0/6`) selected, showing **Welcome to Stow** and six steps: **Save your first tab**, **Let Stow see your Chrome tab**, **Turn on reminders**, **Add the Chrome button (optional)**, **Add an AI key (optional)**, **Voice notes (optional)**. Below it: **Inbox**, **Today**, **Archived**, **All items**, **Buckets**, **Settings**.
- [ ] No macOS permission prompt appears until you click a step (**Allow access**, **Allow notifications**) or press the shortcut.
- [ ] Menu-bar icon shows a monochrome glyph; its menu is **Open Stow**, **Quick Capture ⌃⌘S**, **Paste Link from Clipboard**, **Settings…**, **Quit Stow**.
- [ ] Settings → General → Appearance: Light / Dark / System applies immediately and persists after relaunch.
- [ ] **Dismiss checklist** removes Get set up from the sidebar; Settings → General → **Show setup checklist** brings it back. Completing a step (e.g. first capture) ticks it and bumps the badge; with all six done the entry disappears on its own.
- [ ] Launch the app a second time while it is running (`open -a Stow` or the binary again): no second instance, the existing window comes forward.

## 2. Capture from Chrome
- [ ] With Chrome frontmost on an article, press ⌃⌘S. Panel appears with the URL and title filled, **From Chrome** in the header, bucket picker focused.
- [ ] First time: macOS shows the "Stow wants to control Google Chrome" prompt. Allow → capture proceeds; Settings → Permissions → Chrome Automation reads **Allowed**.
- [ ] Pick **New bucket “Reading”**, press ⏎: **Saved to Reading**, panel hides, Chrome tab closes (**Close tab in Chrome** ticked by default), Chrome is frontmost again.
- [ ] Main window shows the item within seconds with favicon, title, site; status dot turns green (readable copy saved).
- [ ] Capture the same URL again: the panel says **Already saved in Reading** and offers **Keep in Reading**; nothing is duplicated.
- [ ] Switch tabs in Chrome while the panel is open, then save: saved, but the tab is not closed (URL no longer matches) and the panel says so.
- [ ] With a `chrome://` tab frontmost: copy explains the page can't be saved; pasting a URL works.
- [ ] Quit Chrome, press ⌃⌘S: "Chrome isn't running"; a clipboard URL is pre-filled (**From clipboard**) if present. Chrome is **not** launched.
- [ ] Esc hides the panel. Capture a YouTube URL: kind = video, title from YouTube oEmbed, Page tab shows the thumbnail with **Watch on YouTube**, header button reads **Watch**, no snapshot error.
- [ ] Deny Automation (System Settings → Privacy & Security → Automation → Stow → Chrome off): the panel says Stow isn't allowed to control Chrome and lets you paste a link; Settings → Permissions shows **Not allowed** with **Open Privacy Settings**.
- [ ] Untick **Close tab in Chrome** in the panel: Settings → Capture → **Close the tab after saving** follows, and vice versa.

## 3. Add link, paste, import
- [ ] **Add link** (⌘L) above the list opens the inline form with focus in the field; ⌘L again refocuses it; Esc closes it and returns focus to the button. The button is absent in the **Archived** view.
- [ ] With links on the clipboard, the form pre-fills them with a **From clipboard** chip; typing removes the chip.
- [ ] Paste three URLs separated by newlines and one by comma: the counter says **4 links**, the Title field disappears, the button reads **Save 4**; save → four items in the picked bucket and a **Saved 4 to …** toast.
- [ ] Paste one URL already in the library: **Already in <bucket>** (or **Already in Stow (archived)**) with **Show it**; clicking it selects the existing row. In a batch the toast says **… · N already in Stow** with **Show them**.
- [ ] Include a line that isn't a link (`not a link`): the form says **Ignored 1 line that wasn't a web address**, the counter skips it, and only the real links are saved. If one link fails to save (e.g. the database is read-only), the toast says **Couldn't save 1 (see Details)** and the failed line stays in the field.
- [ ] A single link with **Title (optional)** filled saves with that title and the metadata fetch does not overwrite it.
- [ ] ⌘V in the main window with a URL on the clipboard saves into the current bucket (or Inbox) with a toast; pasting a duplicate says "Already in X". ⌘V inside the notes editor pastes text, not a link.
- [ ] Menu bar → **Paste Link from Clipboard** saves to the Inbox and posts a notification (**Saved to Inbox** / **Already saved** / **Nothing to save**).
- [ ] Settings → Capture → **Import all Chrome tabs…**: dialog lists tabs with checkboxes, non-http rows disabled, **Select all** / **Deselect all**, **New bucket “…”** or **Existing:**, **Close imported tabs in Chrome**; **Import N** saves them, closes only the imported tabs if ticked, and the toast reports the count.

## 4. Organize
- [ ] Create, rename (double-click the list title or **Rename**), change emoji, **Move up** / **Move down**, delete a bucket (its items return to Inbox).
- [ ] Move an item via drag to a sidebar bucket, via **Move to…**, and with the M key; the sidebar counts update; **Moved to …** toast.
- [ ] Archive an item (A / hover **Archive** / detail header **Archive** / row menu): it disappears with an **Archived “…”** toast and **Undo** for 6 s; Undo restores it. **Show archived (1)** in the bucket reveals it with an **Archived** chip; **Hide archived** hides it again. The **Archived** view lists it with **Restore** (row, header, and menu); Restore shows a **Restored “…”** toast with Undo.
- [ ] Delete an item (⌫ / **Delete**): **Deleted “…”** toast for 6 s; Undo restores it; otherwise it's gone after relaunch, including snapshot and asset files.
- [ ] Multi-select: ⌘-click two rows, ⇧-click a range, ⌘A all; the bar shows **N selected**, **Archive**, **Move to…**, **Delete**, **Clear** (and **Restore** instead of Archive in the Archived view, where Move to… is absent); each acts on the whole batch with one Undo; Esc clears the selection; A, M, ⌫ act on the selection.
- [ ] Empty states: an empty bucket shows **No links in <bucket> yet** with an **Add link** button; an empty Archived view shows **Nothing archived yet**; a fresh Inbox explains the three ways in.
- [ ] Sort menu (Newest/Oldest/Title/Site) and kind filters (**All**/**Videos**/**Articles**) filter the list.
- [ ] Search (⌘K and ⌘F): typing finds items by title, notes text and page text; each hit shows its type chip and snippet; clicking a hit selects the right bucket and item.
- [ ] Keyboard: ↑/↓/Home/End move selection, ⏎ opens the original, every menu/dialog closes with Esc and returns focus.
- [ ] Resize the window from 1200 px wide down to its 900 × 600 minimum: the list column shrinks (to 360 px under 1100 px), nothing overflows or overlaps, the Add link form drops its hint line on narrow lists, Settings and Today stay readable.

## 5. Notes, tasks, reminders
- [ ] Type notes, switch items quickly, relaunch: nothing lost. **Preview** renders Markdown; images in notes are not loaded.
- [ ] Tasks: add (⏎), toggle, rename (double-click), delete, add a task reminder from the bell.
- [ ] Reminder quick times: **+1h**, **Tonight** (only before the evening), **Tomorrow 9:00**, **Next week**, **This weekend**, **Next Monday** produce the expected local times; **Add reminder** saves.
- [ ] Add a reminder 1 minute out, close the window (app stays in menu bar; first time a toast explains it). A macOS notification appears; clicking it brings Stow forward on that item. Dismiss a second notification and reopen Stow from the Dock within 2 minutes: Stow also lands on that item. The menu-bar menu shows **Due: <title>** (up to three); clicking it navigates too.
- [ ] Today view (⌘2): **Overdue** / **Today** / **Upcoming** sections, **Done**, snooze presets, delete; clicking a row opens the target; overdue count badge on the Today nav item.
- [ ] Reminders list on the item: **Done**, **+1h**, **Tomorrow**, delete.
- [ ] Quit with ⌘Q within a second of typing notes: the edit is saved on relaunch.
- [ ] Leave Stow idle in the background for 10+ minutes with a reminder due in that window: it fires on time (App Nap is disabled).

## 6. Page, files, recordings
- [ ] Page tab shows the readable copy with its status line; **Load images** shows remote images only when on; **Open original** opens the browser; **Download page again** re-downloads (a second click while one runs says it's already running; after a failure the button reads **Try again**).
- [ ] Files: **Add files…** picks and copies; **Open** opens the file; drag a file onto the detail pane; **Delete** removes it (and its folder under `attachments/`).
- [ ] Recordings: first **Record** shows the mic/on-device explainer (**Got it**), macOS mic prompt appears; **Stop** saves; model download progress shows (base.en, 142 MB) the first time; transcript appears with timestamps; playback works; **Transcribe again** works; **Insert into notes** appends.
- [ ] Recording bar is visible on every view while recording; the menu-bar menu shows **Recording… ■ Stop**; switching items mid-recording still attaches the audio to the item you started on.

## 7. AI (needs a key)
- [ ] Settings → AI: paste an Anthropic key into **Anthropic API key**; it shows **Saved** (a Keychain prompt may appear once); **Model** lists known models plus **Custom…**.
- [ ] AI tab: without **I understand** in Settings, the first action shows the **Send to Anthropic?** dialog; **Summarize page** / **Summarize notes** / **Suggest tasks** produce output; **Summarize page** is disabled until a readable copy exists and for videos; **Fix title & description** appears only when the title is just the host name; a suggestion shows the **Pick the tasks to add** preview and **Add N tasks** creates them; **Copy** and **Append to notes** work; output never renders images.
- [ ] **Remove** the key: actions are disabled with a link to **Settings → AI**.

## 8. Extension
- [ ] Settings → Chrome extension → **Install** lists the three steps and **Show extension folder** reveals the bundled folder; load it unpacked (Developer mode). Options → **Pair automatically**: Stow comes forward with **Allow the Chrome extension to save tabs to Stow?** and **Deny** / **Allow**; Allow pairs and the bucket list loads (**Connected to Stow — N buckets**).
- [ ] Alternatively **Copy pairing code**, paste it into the options, **Save**; a wrong code shows **That doesn't look like a pairing code**.
- [ ] Toolbar click on a page saves it (badge ✓); duplicate shows "Already in Stow"; chrome:// page shows ✕ with the reason in the tooltip; **Stow this link** on a link saves the link and never closes the tab.
- [ ] Quit Stow: toolbar click shows "Stow isn't running"; **Reset pairing code** (in the Chrome extension section or the Danger zone) makes the old code fail with "Pairing code changed".
- [ ] Set **Advanced → Extension ID (pins CORS)** to a wrong id: the options page can't load buckets; clear it: works again. A malformed id is rejected inline.

## 9. Settings and data
- [ ] Capture → **Quick capture shortcut**: **Record shortcut**, press e.g. ⌥⇧S, applies immediately, menu-bar menu shows the new key, **Use ⌃⌘S** appears; a conflicting key shows the warning and keeps the old one.
- [ ] Permissions section shows correct states (**Allowed** / **Not allowed** / **Not asked yet**) and the **Ask now** / **Open Privacy Settings** / **Test** buttons work.
- [ ] General → **Open Stow at login** persists (check System Settings → Login Items); the same checkbox in Get set up stays in sync.
- [ ] Data: **Open in Finder**; **Export as Markdown…** creates `Stow export <date>/` with a folder per bucket plus `Inbox/` and `.md` files; **Export as JSON…** writes `stow-export-<date>.json` without `extension_token`; **Back up database…** produces an openable `.db`.
- [ ] General → When I close the window → **Quit Stow** makes the close button quit.
- [ ] Data → Advanced → **Fetch pages on private or local addresses** persists and says a restart is needed.
- [ ] Danger zone → **Delete downloaded models (N MB)** shows the right total, asks to confirm, empties `models/`, and the button then reads **No downloaded models**; Transcription shows **Download now** again.
- [ ] Danger zone → **Delete all Stow data…**: **Delete everything** stays disabled until you type `delete`; confirming shows **Resetting… Stow will restart**, Stow relaunches with an empty library and Get set up at `0/6`, and the data folder contains only fresh files. Esc cancels the prompt.
- [ ] The footer under Data reads **Stow <version>** and, for a CI build, the commit and build date.

## 10. CLI
- [ ] `target/release/stw --help` lists add, ls, buckets, mv, search, rm, done, open, export, backup, doctor, mcp; `stw buckets new qa`; `stw add https://example.com -b qa` prints the item; `stw ls -b qa --json`; `stw search example`; `stw done <id>` then `stw done --undo <id>`; `stw open <id>` prints the URL; `stw export --format markdown /tmp/x`; `stw backup /tmp/x/stow.db`; `stw rm <id>`.
- [ ] `stw doctor` prints the schema version, `integrity: ok`, `search index: ok`, counts, and unfinished jobs and exits 0; `stw doctor --json` is valid JSON. Empty the index behind the app's back (`sqlite3 <data>/stow.db "INSERT INTO search(search) VALUES('delete-all')"`): `stw doctor` reports the search index problem, exits 1, and points at `stw search --rebuild-index`, which repairs it so a following `stw search` finds items again.
- [ ] `stw add chrome://x` exits 1 with "invalid url" and creates no bucket; `stw mv zzz inbox` and `stw ls -b Nope` exit 3; an ambiguous id prefix exits 1 asking for more characters.
- [ ] CLI and app open the same database at the same time without errors; the app shows the CLI's new item after switching views.
- [ ] `stw mcp` configured in Claude Desktop or Claude Code (command `stw`, args `mcp`): the client lists `search`, `get_item`, `add_links`, … and `stow://item/{id}`; a `search` returns hits; `add_links` saves a link the app then shows. With `--read-only` (or `STOW_MCP_READ_ONLY=1`) the writing tools are not listed and calling one is refused. Nothing but protocol traffic appears on stdout.

## 11. Robustness
- [ ] Save a URL that returns a PDF: item saved, Page tab says it couldn't save a readable copy (not an HTML page), no crash.
- [ ] Save `http://localhost:1/x`: saved, readable copy marked "local address not fetched" (unless the Advanced toggle is on and the app restarted).
- [ ] Save a URL that redirects to `http://127.0.0.1/…`: the fetch fails, nothing is requested from the loopback address.
- [ ] Quit normally (⌘Q) while a slow page is still downloading: on relaunch the download resumes and finishes (`stw doctor` shows it pending, not failed).
- [ ] Kill the app mid-fetch (Activity Monitor), relaunch: the item shows "interrupted" or finishes on retry; nothing stays "pending" forever; after three attempts it stops retrying.
- [ ] Delete an item with `stw rm` while its detail pane is open in the app, then add a task, reminder, or file to it: the app shows a "not found" error and no orphan row or file is created (`stw doctor` counts stay consistent).
- [ ] Import 100+ tabs: the app stays responsive; at most a few fetches run at once.
- [ ] Toggle dark mode in macOS with theme = System: the app follows without relaunch.
- [ ] Open the data folder with a newer schema (`PRAGMA user_version = 99`): Stow refuses with a clear message instead of starting.
