# Stow user guide

Stow parks Chrome tabs in buckets so you can close them without losing why you opened them. This guide covers everything in the app; the [README](../README.md) has the short version. Names in **bold** are the exact buttons, menus, and settings you'll see.

## Getting started

1. **Install.** Open the DMG and drag Stow to Applications. Stow needs macOS 13 or later; release builds are for Apple Silicon.
2. **Open Stow.** It puts an icon in the menu bar and opens its main window: navigation and buckets on the left, a list in the middle, details on the right. The sidebar has **Get set up** (until you dismiss it), **Inbox** (⌘1), **Today** (⌘2), **Archived**, **All items**, your **Buckets**, and **Settings** (⌘,).
3. **Get set up.** On first launch the **Get set up** view is selected, with a `0/6` badge in the sidebar. It greets you with **Welcome to Stow** and six optional steps: **Save your first tab** (press the shortcut, or **Add link**), **Let Stow see your Chrome tab** (**Allow access** asks for the Automation permission), **Turn on reminders** (**Allow notifications**, plus an **Open Stow at login** checkbox), **Add the Chrome button**, **Add an AI key**, and **Voice notes** (**Download now** fetches the Whisper model). Steps tick themselves as you use the features. **Dismiss checklist** hides the view; it also disappears once every step is done. Bring it back with Settings → General → **Show setup checklist**.
4. **Closing the window does not quit Stow.** It keeps running in the menu bar so reminders fire and the shortcut works (a toast explains this the first time you reopen the window). Quit from the menu-bar icon (**Quit Stow**) or with ⌘Q. If you'd rather it behaved like a normal app, choose Settings → General → When I close the window → **Quit Stow**. **Open Stow at login** is in the same section. Opening Stow while it is already running just brings the existing window forward.

The menu-bar icon's menu has **Open Stow**, **Quick Capture** (shows the current shortcut), **Paste Link from Clipboard**, up to three **Due: …** entries for reminders that have come due (click one to jump to it), **Recording… ■ Stop** while a recording is running, **Settings…** (⌘,), and **Quit Stow** (⌘Q).

## Permissions

macOS asks for each permission the first time Stow needs it. Nothing is requested up front.

| Permission | Why | When it's requested | Where to change it |
|---|---|---|---|
| Automation → Google Chrome | Read the front tab's URL and title; close the tab after capture | First ⌃⌘S, or **Allow access** in Get set up | System Settings → Privacy & Security → Automation → Stow → Google Chrome |
| Notifications | Reminders | First reminder, or **Allow notifications** in Get set up | System Settings → Notifications → Stow |
| Microphone | Recordings | First **Record** | System Settings → Privacy & Security → Microphone → Stow |

Settings → **Permissions** lists **Chrome Automation**, **Notifications**, and **Microphone** with their status (**Allowed**, **Not allowed**, **Not asked yet**, or **Chrome isn't running**). **Ask now** appears while a permission hasn't been asked for yet, **Open Privacy Settings** opens the right System Settings pane, and **Test** (Automation only) reads the current Chrome tab to prove it works. If Automation is denied, the capture panel says so and lets you paste a link instead.

## Capturing links

**Shortcut.** With Chrome in front, press ⌃⌘S. The capture panel shows the tab's address and title with the bucket picker focused. Start typing a bucket name to filter, or pick **New bucket “…”** to create one. Press ⏎ to save. **Close tab in Chrome** is ticked by default (it mirrors **Close the tab after saving** in Settings → Capture); Stow closes the tab only if it still shows the address that was captured. Esc hides the panel. If the address is already in your library, the panel says **Already saved in …** and offers **Keep in …**; move it from the main window if it belongs elsewhere. If Chrome isn't running, the panel says so and pre-fills a link from the clipboard when there is one. Chrome is never launched by Stow.

Change the key combination under Settings → Capture → **Quick capture shortcut**: click **Record shortcut**, press the keys, done (⌫ while recording resets to ⌃⌘S, and **Use ⌃⌘S** appears whenever you've changed it). If macOS refuses to register a combination because another app owns it, Stow says so and keeps the previous one.

**Add link.** Click **Add link** above the list, or press ⌘L (⌘L again puts the caret back in the field). Paste one link or many, separated by spaces, commas, or new lines; the form counts the links it found (and notes any lines it ignored because they weren't web addresses), and when the clipboard already holds links it pre-fills them with a **From clipboard** chip. With a single link you can set a **Title (optional)**. Choose **Save to bucket** (the list you're viewing is pre-selected; Inbox otherwise) and press ⏎ or click **Save** / **Save N**. ⇧⏎ adds a new line, Esc cancels. A link you already have is reported (**Already in …**, with **Show it**) instead of being duplicated; in a batch the toast reads **Saved N to … · M already in Stow**, and links that couldn't be saved stay in the field with the reason under **Details**. Add link is not offered in the Archived view.

**Paste.** Press ⌘V anywhere in the main window with a link on the clipboard. It goes to the bucket you're viewing, or the Inbox. The menu-bar menu's **Paste Link from Clipboard** saves to the Inbox and confirms with a notification (**Saved to Inbox**, **Already saved**, or **Nothing to save**).

**Import all tabs.** Settings → Capture → **Import all Chrome tabs…** lists every open Chrome tab with checkboxes (pages that aren't `http`/`https` are disabled), **Select all** / **Deselect all**, a destination (**New bucket “…”** or **Existing:**), and **Close imported tabs in Chrome**. **Import N** saves them; only the imported tabs are closed.

**Extension.** A toolbar button and right-click menu in Chrome; see [extension.md](extension.md).

**CLI.** `stw add <url> -b <bucket>`; see [below](#the-stw-cli).

**Duplicates.** Stow compares *canonical* URLs: the scheme and host are lowercased, the fragment (`#…`) is dropped, tracking parameters (`utm_*`, `fbclid`, `gclid`, `ref`, `si`, and friends) are removed, and YouTube URLs are reduced to `https://www.youtube.com/watch?v=ID`. Saving a URL you already have selects the existing item instead of adding a second one; if it was in the Inbox and you picked a bucket, it moves there. Only `http` and `https` links can be saved.

**What happens after saving.** Saving is instant and never waits on the network. In the background Stow fetches the title, description, site name, favicon, and preview image, then a readable copy of the article (not for video pages, which get a thumbnail and a **Watch on YouTube** / **Open video** button instead). A status dot on the row turns green when the copy is saved. Links to `localhost` or private network addresses are saved but not fetched; see [Advanced](#settings-reference).

## Organizing

**Buckets and the Inbox.** A bucket is an intent: "watch for the business", "blog sources", "training to finish". Each link belongs to exactly one bucket, or to the Inbox (⌘1) if you haven't sorted it. **All items** shows everything. Create buckets with the sidebar's **New bucket** (⌘N), from the capture panel, from Add link, or with the CLI. Give a bucket an emoji and a description (double-click the list title, or **Rename** / **Change emoji** from its menu); **Move up** / **Move down** reorder them.

**Moving.** Drag a link onto a bucket in the sidebar, press M, or use **Move to…** from the row's menu or the bulk bar. Deleting a bucket moves its links back to the Inbox.

**Archiving.** When you're done with a link, press A, or hover the row and click **Archive** (also in the detail header and the row's menu). The link leaves the list and appears under **Archived** in the sidebar with an **Archived** chip; **Restore** brings it back. Inside a bucket, **Show archived (N)** reveals its archived links and **Hide archived** tucks them away again. Archiving is undoable for six seconds from the toast.

**Deleting.** ⌫, or **Delete** on a row or in the detail header, removes a link with its readable copy, favicon and preview, attachments, recordings, AI runs, reminders, and tasks. An **Undo** toast gives you six seconds to take it back. Deleting a bucket removes everything attached to the bucket itself and moves its links to the Inbox.

**Several at once.** ⌘-click to toggle links, ⇧-click for a range, ⌘A for every visible row. A bar shows **N selected** with **Archive** (or **Restore** in the Archived view), **Move to…**, and **Delete** for the whole batch, with one Undo for all of them; **Clear** or Esc drops the selection. A, M, and ⌫ act on the selection too.

**Sorting and filtering.** The list header has a sort menu (Newest, Oldest, Title, Site) and kind filters (**All**, **Videos**, **Articles**).

## Working with a link or bucket

Select a link to open its detail pane: title, address, site, kind, bucket, and when it was saved, with **Open original** (**Watch** for videos), **Archive** / **Restore**, **Delete**, and a menu with **Download page again** and **Delete…**. Tabs: **Notes**, **Page** (links only), **Tasks**, **Reminders**, **Files**, **Recordings**, **AI**. Selecting a bucket with no link open shows the same tabs for the bucket itself.

**Notes.** Markdown with **Edit** and **Preview**. Notes autosave as you type and are flushed when you switch items or quit. Images, forms, and embeds in notes are never rendered.

**Page.** The readable copy of the article, with a status line (**Readable copy from …**, **Saving readable copy…**, or the error). **Download page again** fetches the metadata and the copy again (**Try again** after a failure). Remote images are shown as links unless you turn on **Load images** for that item; the toggle is per item and off by default because loading images contacts the sites that host them.

**Tasks.** A checklist on every bucket and link: **Add a task and press Enter**, tick off, double-click to rename, delete. Each task can carry its own reminder (the bell).

**Reminders, Files, Recordings, AI**: below.

## Reminders and Today

In the **Reminders** tab, click **Reminder**, pick a quick time (**+1h**, **Tonight** while the evening is still ahead, **Tomorrow 9:00**, **Next week**, **This weekend**, **Next Monday**) or a date and time, add an optional message, and **Add reminder**. When it's due, macOS shows a notification (the message, or the link's title). Clicking the notification opens Stow on the target; so does opening Stow within two minutes of it firing. In the Reminders list each reminder has **Done**, **+1h**, **Tomorrow**, and delete.

**Today** (⌘2) lists **Overdue**, **Today**, and **Upcoming** reminders, with an overdue count badge in the sidebar. Each row has **Done**, snooze presets (**+1h**, **Tonight**, **Tomorrow 9:00**, **Next week**), and delete; click the row to jump to the link, bucket, or task. Reminders are checked every 30 seconds while Stow is running, so keep it in the menu bar if you want them to fire. Stow also keeps macOS from putting it to sleep (App Nap) while it waits.

## Recordings and transcripts

Click **Record** on any bucket or link to capture a voice memo (the first time, a note explains that audio stays on this Mac; **Got it**). A red bar stays visible on every view while recording; click **Stop** to finish. The audio is attached to the item you started on even if you switch items meanwhile. Audio is saved as 16 kHz mono WAV and transcribed on your Mac with whisper.cpp (Metal on Apple Silicon). The transcript appears with timestamps and becomes searchable; **Insert into notes** appends it to the notes, **Transcribe again** re-runs it with the current model.

The first transcription downloads the selected Whisper model and shows progress. Settings → **Transcription** lists **tiny.en — 75 MB · fastest**, **base.en — 142 MB · recommended** (the default), and **small.en — 466 MB · most accurate**, and offers **Download now** for the selected one. Only English models are offered. Downloads come from a pinned whisper.cpp revision and are verified by size and SHA-256 before use. **Delete downloaded models** in Settings → Data → Danger zone frees the space; the next transcription downloads again.

## AI actions

Settings → **AI**: choose **Provider** (Anthropic or OpenAI), a **Model** (the provider's current default is preselected; **Custom…** takes any model id), and paste your key into **Anthropic API key** or **OpenAI API key**. The key is saved to the macOS Keychain and never shown again (**Saved**); **Remove** deletes it. The section also explains what is sent, with an **I understand** button; if you haven't clicked it, the first AI action asks instead with a **Send to Anthropic?** / **Send to OpenAI?** dialog.

The **AI** tab of a link or bucket offers:

| Action | What is sent to the provider |
|---|---|
| **Summarize page** (links; needs a readable copy, not offered for videos) | Title, URL, and the readable copy of the page |
| **Summarize notes** (needs notes) | Title, bucket name, and the bucket's or link's notes |
| **Summarize recording** (needs a transcript) | Title and the most recent transcript |
| **Suggest tasks** | Title, URL, bucket name, your notes, and the latest transcript (not the page text) |
| **Fix title & description** (links whose title is just the site's address) | Title, URL, and the page text; returns title, description, and site name |

Page text and transcripts are cut to 60,000 characters and marked as untrusted so instructions hidden in a web page cannot steer the model. Results are saved as AI runs under the item or bucket with the provider and model recorded; each has **Copy** and **Append to notes**, and a task suggestion shows a **Pick the tasks to add** preview with **Add N tasks**. Nothing is sent until you click an action, and usage is billed to your own account.

## Search

Press ⌘K (or ⌘F) or click **Search ⌘K**. Search covers bucket names and notes, link titles and descriptions, notes, readable copies, transcripts, and AI output; each word matches as a prefix, and titles rank higher. Each result shows what matched (item, snapshot, recording, …), the title, and a snippet; select one to jump there. If results look stale, `stw search --rebuild-index` rebuilds the index.

## The `stw` CLI

Build with `cargo build -p stow-cli --release` from a checkout (the binary is `target/release/stw`; `cargo install --path crates/stow-cli` puts it on your `PATH`), or download `stow-cli_<version>_aarch64-apple-darwin.tar.gz` from the release. It uses the same data folder as the app (both can be open at once); the app shows CLI changes when you switch views.

```bash
stw add https://example.com/article -b blog      # save into bucket "blog" (created if missing); fetches metadata + readable copy
stw add <url> --no-fetch --title "My title"       # skip fetching; set the title yourself
stw ls                                           # Inbox; -b <bucket> for one bucket, --all for everything, --json for JSON
stw buckets                                      # list buckets with counts; `stw buckets new "name"` creates one
stw mv <item> <bucket>                           # move an item (id or unique id prefix); `inbox` moves it to the Inbox
stw search whisper metal                         # full-text search, up to 25 hits; --rebuild-index repairs a drifted index first
stw done <item>                                  # archive; `stw done --undo <item>` restores
stw open <item>                                  # print the URL; --browser opens it
stw rm <item>                                    # delete with its notes, tasks, files, and recordings
stw export --format markdown ~/Desktop           # Markdown folders; --format json for one JSON file
stw backup ~/Desktop/stow.db                     # consistent copy of the database, safe while the app runs
stw doctor                                       # schema version, integrity check, search index, counts, unfinished jobs; --json
stw mcp                                          # MCP server over stdio for Claude Desktop / Claude Code; --read-only hides writing tools
stw --version
```

Items are referred to by id or by a unique prefix of the id. Exit code 1 means an error (including `stw doctor` finding a problem), 3 means the bucket or item was not found. Set `STOW_DATA_DIR` (or pass `--data-dir`) to use a different data folder; `--allow-local-urls` (or `STOW_ALLOW_LOCAL_URLS=1`) lets `add` fetch `localhost` and private addresses. The CLI cannot record, transcribe, or run AI actions; those need the app.

**MCP.** `stw mcp` serves your library to an MCP client over stdin/stdout: point Claude Desktop or Claude Code at the `stw` binary with the argument `mcp` (add `--data-dir` if you use a custom folder). The client gets `search`, `list_buckets`, `list_items`, `get_item` (everything about a link, including its readable text), `doctor`, and the resources `stow://item/{id}` and `stow://bucket/{id}`; unless you pass `--read-only` or set `STOW_MCP_READ_ONLY=1` it also gets `add_links`, `move_item`, `archive_item`, `delete_item`, `update_notes`, `add_task`, `complete_task`, `add_reminder`, `create_bucket`, and `export`. Links added this way are deduplicated like any other, and `add_links` downloads the title and readable text right away unless the client passes `fetch: false` (then the app fetches them on its next launch).

## Settings reference

| Section | Options |
|---|---|
| **General** | **Appearance** (Light, Dark, System; Light is the default), **Open Stow at login**, **When I close the window** (**Keep Stow in the menu bar** / **Quit Stow**), **Show setup checklist** |
| **Capture** | **Quick capture shortcut** (default ⌃⌘S) with **Record shortcut**, **Use ⌃⌘S**, and its registration status; **Close the tab after saving**; **Import all Chrome tabs…** |
| **Permissions** | **Chrome Automation**, **Notifications**, **Microphone** with status, **Ask now** (until asked), **Open Privacy Settings**, **Test** (Automation) |
| **Chrome extension** | **Pairing code** with **Copy pairing code** and **Reset pairing code**; **Install** steps with **Show extension folder**; **Advanced** → **Extension ID (pins CORS)** |
| **AI** | **Provider**, **Model** (known models or **Custom…**), **Anthropic API key** / **OpenAI API key** (**Saved** / **Remove**), the consent note with **I understand** |
| **Transcription** | **Transcription model** (tiny.en, base.en, small.en with sizes), **Downloaded ✓** or **Download now** |
| **Data** | **Data folder** with **Open in Finder**; **Export**: **Export as Markdown…**, **Export as JSON…**, **Back up database…**; **Advanced**: **Fetch pages on private or local addresses (e.g. http://localhost)** (off by default, takes effect after restart); **Danger zone**: **Reset pairing code**, **Delete downloaded models (N MB)**, **Delete all Stow data…** (type `delete`, then **Delete everything**; Stow restarts with an empty library) |

The footer under Data shows the build: **Stow <version>**, the commit it was built from, and the build date.

## Your data and backups

Everything is in `~/Library/Application Support/Stow/` (see the table in the [README](../README.md#privacy-and-data)). API keys are in the Keychain (service `io.github.chaosdeselva.stow`), not in this folder.

- **Time Machine** covers the folder automatically.
- **Back up database…** (Settings → Data, or `stw backup`) writes a consistent copy of `stow.db` while Stow is running, using SQLite's online backup. Copy the other folders as they are.
- **Export as Markdown…** writes `Stow export <date>/` with one folder per bucket (plus `Inbox/`), a `README.md` per bucket with its notes and tasks, and one `.md` per link with front matter (title, url, saved, kind, done, bucket) and sections for Notes, Tasks, Transcripts, AI notes, and Files, with attachments copied alongside. **Export as JSON…** writes `stow-export-<date>.json` with every table (ids preserved) except the extension token. A second export on the same day gets a numeric suffix rather than overwriting.
- **Moving to a new Mac.** Copy the whole folder into place before launching Stow, then re-enter your API keys and re-pair the extension.
- **Starting over.** Settings → Data → Danger zone → **Delete all Stow data…** empties the folder and restarts Stow. Keys stay in the Keychain; the extension needs pairing again.
- **Uninstalling.** Delete `Stow.app`, the data folder, and the `io.github.chaosdeselva.stow` items in Keychain Access. Chrome keeps the extension until you remove it there.

## Keyboard shortcuts

| Keys | Action |
|---|---|
| ⌃⌘S (configurable) | Quick capture from Chrome, anywhere on the Mac |
| ⌘L | Add link |
| ⌘V | Paste a link from the clipboard |
| ⌘K, ⌘F | Search |
| ⌘N | New bucket |
| ⌘1, ⌘2, ⌘, | Inbox, Today, Settings |
| ↑ ↓ Home End | Move through the list |
| Enter | Open the original page |
| A | Archive (or Restore) the selected link(s) |
| M | Move the selected link(s) to a bucket |
| ⌫ | Delete the selected link(s), with Undo |
| ⌘-click, ⇧-click, ⌘A | Select several links |
| Esc | Clear the selection; close menus, dialogs, and the capture panel |

## Troubleshooting

**The capture panel says Stow isn't allowed to control Chrome.** Automation for Chrome was denied. Open System Settings → Privacy & Security → Automation, find Stow, and enable Google Chrome (Settings → Permissions → **Open Privacy Settings** takes you there). If Stow isn't listed, press the shortcut once with Chrome in front so macOS asks. Builds run from a terminal (`tauri dev`) attribute the permission to the terminal, not Stow.

**The shortcut does nothing.** Settings → Capture shows a warning if another app owns the combination. Record another one. Also check that Stow is running (menu-bar icon).

**No notifications.** System Settings → Notifications → Stow → Allow Notifications. Stow must be running in the menu bar for reminders to fire; if you chose "Quit Stow" when the window closes, nothing fires while it's closed. Unsigned development builds may not be able to post notifications at all.

**Recording fails or is silent.** Grant Microphone access (System Settings → Privacy & Security → Microphone). On Macs with no input device, recording isn't available. Check the input device in System Settings → Sound.

**The model download never finishes or fails verification.** Check your connection and free disk space (`small.en` needs about 490 MB). Delete any `models/ggml-*.bin.part` file in the data folder (or use **Delete downloaded models**) and retry; a file that fails its size or SHA-256 check is removed automatically and downloaded again.

**Transcription is slow.** Pick a smaller model, or check Activity Monitor: the first run after a download compiles the Metal shaders. Intel Macs transcribe on the CPU only.

**A saved page has no readable copy.** Some pages can't be extracted: PDFs and other non-HTML responses, pages over 10 MiB, pages that render everything in JavaScript, and video pages (by design). The item keeps its metadata; **Download page again** / **Try again** tries once more. "local address not fetched" means the link points at `localhost` or a private address; turn on Settings → Data → Advanced → **Fetch pages on private or local addresses** and restart if you want those fetched. "interrupted" means Stow was killed mid-fetch (a normal quit resumes the work next launch); after three failed attempts Stow stops retrying.

**Search misses something, or the app misbehaves after a crash.** Run `stw doctor`: it checks the database, the search index, and lists unfinished background work. `stw search --rebuild-index` rebuilds the index from the stored documents.

**The extension shows ✕.** See [extension.md](extension.md#troubleshooting).

**Logs.** Quit Stow, then run `RUST_LOG=debug /Applications/Stow.app/Contents/MacOS/stow-app` in Terminal and reproduce the problem. Logs may contain URLs and titles; keys are never logged. Bug reports go to the [issue tracker](https://github.com/ChaosDeSelva/Stow/issues/new/choose).
