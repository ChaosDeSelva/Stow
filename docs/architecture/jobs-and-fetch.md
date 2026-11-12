# Jobs and fetching

How Stow does work in the background, and the rules that keep a saved link from turning into a request against your own network. Everything here is read from `crates/stow-core/src/jobs.rs`, `fetch/{mod,net,metadata,snapshot}.rs`, `transcribe.rs`, `lib.rs`, and the app's `lib.rs`.

## The capture path never touches the network

`Stow::capture(CaptureRequest { url, bucket_id, title })` does exactly this, synchronously:

1. `capture::normalize_url` — trim, require `http`/`https`, lowercase scheme and host, drop the fragment, strip tracking parameters (`utm_*`, `fbclid`, `gclid`, `dclid`, `msclkid`, `mc_cid`, `mc_eid`, `igshid`, `ref`, `ref_src`, `si`, `feature`, `t_src`, and `s`/`t` on x.com/twitter.com), canonicalize YouTube URLs to `https://www.youtube.com/watch?v=ID[&t=…]`. The result is the `canonical_url` dedupe key.
2. `items::create` — one transaction: `INSERT … ON CONFLICT(canonical_url) DO NOTHING`. On conflict the existing row is returned with `created: false` and, if it sat in the Inbox and a bucket was requested, moved there. The item's `search_docs` row is written in the same transaction.
3. If the URL is local (see below) and local fetching is off, the item is marked `snapshot_status = failed`, `snapshot_error = "local address not fetched"` and no jobs are queued.
4. Otherwise, for a *new* item, `Job::FetchMetadata` and `Job::FetchSnapshot` are enqueued. A duplicate enqueues nothing: its metadata was fetched when it was first saved.
5. `ItemAdded` (new) or `ItemUpdated` (existing) is sent through the job hook so every window refreshes.

The CLI has no queue; `stw add` runs `Stow::fetch_item` inline unless `--no-fetch`, and prints a warning rather than failing if the metadata fetch errors.

## The job queue

`JobQueue` is one tokio task reading from a bounded channel, spawning a task per job.

| Setting | Value | Why |
|---|---|---|
| `QUEUE_CAPACITY` | 1024 | `enqueue` drops and logs when the channel is full; the UI is never blocked |
| `NETWORK_SLOTS` | 4 | concurrent metadata/snapshot fetches (importing 300 tabs no longer opens 600 connections) |
| `TRANSCRIBE_SLOTS` | 1 | whisper.cpp already uses up to 8 threads and holds a model in memory |
| `AI_SLOTS` | 2 | concurrent provider calls |
| `SHUTDOWN_GRACE` | 5 s | how long `shutdown()` waits for running jobs at quit |
| `MAX_ATTEMPTS` | 3 | snapshot or transcription starts before Stow gives up |

Job kinds: `FetchMetadata { item_id }`, `FetchSnapshot { item_id }`, `Transcribe { recording_id }`, `Ai { run_id }`.

**Dedupe.** `enqueue` keeps a set of in-flight jobs (queued or running). An identical job returns `false` and is not queued. The Tauri commands `refetch_item` and `retranscribe` turn that `false` into the `busy` error ("That page is already being fetched." / "That recording is already being transcribed."). The entry is cleared when the task ends, even by panic.

**Events.** Jobs report through a `JobHook` (`BucketsChanged`, `ItemAdded`, `ItemUpdated`, `RecordingUpdated`, `AiRunUpdated`, `Progress { kind, received, total }`). The app forwards them to windows as the events in the IPC contract; `Progress` with `kind: "whisper_model"` becomes `model-download-progress`.

**Options.** `JobQueueOptions { allow_local_urls }` is read once at startup from the `allow_local_urls` setting, which is why the Settings toggle says a restart is required. The queue builds its HTTP client from it (`fetch::client_for`).

## Resume after a restart

`jobs::resume_pending` runs once at app start, in one transaction:

1. Rows still `running` were cut off mid-job (quit, crash, or the job itself crashing the process). They become `failed` with error `interrupted`; items and recordings also get their attempt counter (`snapshot_attempts`, `transcript_attempts`) incremented. They are **not** retried blindly: a page that crashes the parser must not crash the app on every launch.
2. `pending` snapshots and transcriptions whose counter is already `>= MAX_ATTEMPTS` become `failed` with `gave up after 3 attempts`.
3. Every remaining `pending` snapshot, transcription, and AI run is re-enqueued, oldest first.

Metadata jobs have no status column and are not resumed; the user can **Refetch**. `items::start_snapshot` and `recordings::start_transcript` are what bump the attempt counters when a job begins.

At quit (`RunEvent::Exit`) the app calls `JobQueue::shutdown()`: it stops accepting jobs, waits up to 5 s for running ones, and logs if some were still going. Queued jobs that never started stay `pending` in the database and are picked up by `resume_pending` next launch.

## What each job does

**FetchMetadata.** `metadata::fetch_metadata`: for YouTube hosts, `GET https://www.youtube.com/oembed?url=…&format=json` and parse title, author (`site_name` becomes `YouTube · author`), and thumbnail. Otherwise `GET` the page; a non-HTML response yields only `site_name` (the host). HTML is read with `read_text_capped` (10 MiB cap, charset from `Content-Type`), nesting-checked, then parsed with `scraper`: `og:title` else `<title>`, `og:description` else `meta[name=description]`, `og:site_name` else host, `link[rel~=icon]` else `/favicon.ico`, `og:image` else `twitter:image`. The favicon and preview image are then downloaded with `download_asset` into `assets/<item-id>-favicon.<ext>` and `assets/<item-id>-image.<ext>`; a failed asset is logged and skipped. `items::set_metadata` merges the result; a fetched title only replaces the host-name placeholder, never a title the user typed. Errors leave the item untouched.

**FetchSnapshot.** Video items (`kind = video`) get `snapshot_status = none` and no fetch: a readable copy of a player page is just chrome. Otherwise `start_snapshot` marks it `running` (attempt +1), `snapshot::fetch_snapshot` GETs the page (must be HTML, 10 MiB cap), `check_nesting` runs, `dom_smoothie` readability extracts the article, `htmd` converts it to Markdown (skipping `script`, `style`, `nav`, `aside`, `footer`, `iframe`, `noscript`), and a plain-text version is derived for search. `snapshots/<item-id>.html` and `.md` are written, then `finish_snapshot` sets `done` and indexes the `snapshot` search doc in one transaction. Any error becomes `failed` with the error text (`not an HTML page`, `too large`, `nesting too deep`, HTTP errors, …).

**Transcribe.** `start_transcript` marks `running`; the model named by the `whisper_model` setting is validated against the allowlist, `transcribe::ensure_model` downloads it if missing (progress events), `WhisperModelStore` loads it (cached across runs; loading a different model replaces the cached one), whisper.cpp runs on a blocking thread with `min(available cores, 8)` threads and language `en`, and the result is stored as Markdown (`**[mm:ss]** text` per segment) via `set_transcript`, which also indexes the `recording` search doc. Failures record the error on the recording.

**Ai.** `ai::run_ai` builds the provider from settings and the Keychain (a missing key fails the run with a `provider` error before any request), loads context (`load_context`: title, URL, bucket name, notes, snapshot Markdown, latest transcript), builds the prompt (`prompts::build_prompt`), calls the provider with a 15 s connect / 120 s total timeout, and stores the trimmed reply on the `ai_runs` row (`complete`, which indexes the `ai_run` search doc). Page text and transcripts are truncated to 60,000 characters and wrapped in `<untrusted_page>` / `<untrusted_transcript>`; `SuggestTasks` never includes page text.

## HTTP client and SSRF rules

All page and asset fetches share one `reqwest::Client` built by `fetch::client()`:

- Desktop Safari user agent with a `Stow/0.1` suffix (some sites serve bots a stripped page); 15 s total timeout.
- **`no_proxy()`** — system proxies are ignored. A proxy resolves names itself, which would route around every check below.
- **`SafeResolver`** — names are resolved with the system resolver (`getaddrinfo`), then every address that is not `is_public_ip` is dropped. A name with no public address left is an error, so the request never connects.
- **`PublicPeerLayer`** — after each connection is established, the actual peer address is read from the connector and must be public. This catches literal-IP URLs (which bypass the resolver) and anything else that would reach a non-public peer; no request bytes are sent otherwise.
- **Redirect policy** — each hop must be `http`/`https`, must not be a local URL (`is_local_url`), and at most `MAX_REDIRECTS = 10` hops are followed.

`is_public_ip` is false for: loopback, RFC 1918 private, link-local (incl. `169.254.169.254`), CGNAT `100.64.0.0/10`, multicast, broadcast, unspecified, `0.0.0.0/8`, `192.0.0.0/24`, benchmark `198.18.0.0/15`, documentation ranges, `240.0.0.0/4`; for IPv6: loopback, unspecified, multicast, ULA `fc00::/7`, link-local `fe80::/10`, site-local `fec0::/10`, documentation `2001:db8::/32`, Teredo `2001::/32`, IPv4-compatible `::a.b.c.d`, and the IPv4-mapped, NAT64 (`64:ff9b::/96`) and 6to4 (`2002::/16`) forms of any non-public IPv4 address.

`is_local_url` (used before fetching and on redirects) is true for `localhost`, `*.localhost`, a literal IP that is not public, or a URL with no host. Hostnames are not resolved here; that is the resolver's job.

**Local URL policy.** A captured local URL is always *saved*: the user may want the bookmark. With `allow_local_urls` off (the default) it is never fetched, its snapshot is marked failed with `local address not fetched`, and no jobs are queued. With it on (Settings → Data → Advanced, or `stw --allow-local-urls` / `STOW_ALLOW_LOCAL_URLS`), `fetch::client_allow_local()` is used instead: same UA and timeout, plain 10-hop redirect limit, no resolver or peer check. Tests use that client because wiremock listens on `127.0.0.1`.

The Whisper model download uses a separate `transcribe::download_client()` (15 s connect, 60 s per-read stall timeout, no total timeout, 10 redirects) because a 488 MB download cannot finish inside the 15 s shared timeout; its destination is the pinned `huggingface.co` URL, not user input. AI provider clients are built in `ai::select_provider` with their own timeouts.

## Size and parsing caps

| Cap | Value | Where |
|---|---|---|
| Page body (metadata and snapshot) | 10 MiB (`MAX_PAGE_BYTES`) | `read_capped`: refused up front when `Content-Length` says so, otherwise the stream is abandoned the moment the running total exceeds it; never more than the cap is buffered |
| Asset (favicon, preview) | 5 MiB (`MAX_ASSET_BYTES`) | same streaming check; the file extension comes only from the `Content-Type`, which must be `png`, `jpeg`, `gif`, `webp`, `svg+xml`, `x-icon`/`vnd.microsoft.icon`, or `avif`; anything else is `not an image`, so a server cannot plant an `.html` or `.app` in the data dir |
| Element nesting | 512 levels (`MAX_NESTING`) | `check_nesting`: a cheap tag counter run before readability and metadata parsing; void and optional-end-tag elements don't count, comments and raw-text elements are skipped, `/>` only self-closes inside `<svg>`/`<math>` |
| Parser stack | 64 MiB | `on_parser_thread`: readability and HTML→Markdown recurse per element and run on their own thread, off tokio's small-stack workers |
| Model file | exact size and SHA-256 from `MODELS` | checked while streaming to `<path>.part`; mismatch deletes the `.part` and reports `checksum mismatch` |
| Extension endpoint | 16 KiB body, 8 KiB URL, 1 KiB title, 100-char pair name | `server.rs` (see [SECURITY.md](../../SECURITY.md)) |

## Files written by jobs

| Job | Files |
|---|---|
| FetchMetadata | `assets/<item-id>-favicon.<ext>`, `assets/<item-id>-image.<ext>` |
| FetchSnapshot | `snapshots/<item-id>.html`, `snapshots/<item-id>.md` |
| Transcribe | `models/ggml-<name>.bin` (via `.part`); reads `audio/<recording-id>.wav` |
| Ai | none (output is in `ai_runs.output_md`) |

Deleting an item removes `snapshots/<id>.*`, every `assets/<id>-*` file, and the attachment and audio files of its child rows (`OwnedFiles`, collected inside the delete transaction and removed after it commits).

## Reminders (the other background loop)

Not a job, but the same shape: `scheduler::start` ticks every 30 s, calls `stow_core::scheduler::fire_due(now)`, which marks each due reminder fired **before** notifying so a crash in the notifier can never double-fire, and posts a macOS notification (heading = target title if the reminder has a message, else "Reminder"; body = message or title). The target is remembered for 60 s so that reopening the app counts as clicking the notification and navigates there.
