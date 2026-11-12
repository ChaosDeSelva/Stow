# MCP server (`stw mcp`)

Stow ships a [Model Context Protocol](https://modelcontextprotocol.io) server, so an AI assistant such as Claude Desktop or Claude Code can search your library, read the saved pages, file new links into buckets, and keep notes and tasks up to date, all against the same database the app uses.

`stw mcp` runs the server over stdio: the MCP client starts `stw` as a child process and talks JSON-RPC on its stdin/stdout. Nothing listens on a port. The code lives in `crates/stow-mcp` (library) and is wired in by `crates/stow-cli`.

## Install

Build or install `stw` (`cargo install --path crates/stow-cli`, or use the binary bundled with the app) and point the client at it. `STOW_DATA_DIR` is only needed when the library is not in the default location (`~/Library/Application Support/Stow`).

**Claude Desktop** — add to `claude_desktop_config.json` (Settings → Developer → Edit Config):

```json
{
  "mcpServers": {
    "stow": {
      "command": "/path/to/stw",
      "args": ["mcp"],
      "env": { "STOW_DATA_DIR": "/Users/you/Library/Application Support/Stow" }
    }
  }
}
```

Drop the `env` block to use the default data directory. For a read-only connection, add `"--read-only"` to `args` (or `"STOW_MCP_READ_ONLY": "1"` to `env`).

**Claude Code:**

```sh
claude mcp add stow -- stw mcp
# read-only:
claude mcp add stow -- stw mcp --read-only
```

Restart the client; `stow` then appears among its tools. Ask something like "what did I save about SQLite last week?" or "park these three links in my Reading bucket".

## Tools

| Tool | What it does |
|---|---|
| `search { query, limit? }` | Full-text search across titles, notes, page text, transcripts, and AI notes; hits carry `doc_type`, `item_id`, `bucket_id`, `title`, `snippet`. |
| `list_buckets {}` | Buckets in sidebar order with `item_count`, `open_count`, plus `inbox_count`. |
| `list_items { bucket?, include_archived?, limit? }` | Items newest first: `id`, `title`, `url`, `kind`, `bucket`, `saved_at`, `archived`. `bucket` is an id, a name, `inbox` (default), or `all`. |
| `get_item { id, include_text? }` | Everything about one link: metadata, bucket, notes, tasks, reminders, readable text (Markdown, cut at 60,000 characters with a marker), transcripts, AI notes. |
| `add_links { urls[], bucket?, fetch? }` | Save URLs (deduplicated by canonical URL) into a bucket or the Inbox. With `fetch` (default true) the title, metadata, and readable text are downloaded immediately; with `fetch: false` the snapshot is marked pending and the Stow app fetches it on its next launch. One result per URL: `id`, `created`, `title`, or `error`. |
| `move_item { id, bucket? }` | Move an item to a bucket (id or name); omit the bucket or pass `inbox` for the Inbox. |
| `archive_item { id, archived }` | Mark done/archived, or reopen. |
| `delete_item { id }` | Delete an item with its notes, tasks, reminders, files, recordings, and AI notes. |
| `update_notes { id \| bucket_id, mode, markdown }` | `append` to or `replace` the Markdown notes of an item or a bucket. |
| `add_task { parent_type, parent_id, title }` | Add a checklist task to an `item` or a `bucket`. |
| `complete_task { id, done }` | Tick a task off or reopen it. |
| `add_reminder { target_type, target_id, at, message? }` | Schedule a reminder on an `item`, `bucket`, or `task` at an RFC 3339 time; the app shows the notification. |
| `create_bucket { name, emoji? }` | New bucket. |
| `export { format, dest_dir }` | Export the library as `markdown` folders or one `json` file into `dest_dir`. |
| `doctor {}` | SQLite integrity, search-index consistency, row counts, unfinished jobs. |

Every tool returns structured JSON (also serialized into the text content). Failures are tool-level errors whose text starts with the stable `stow-core` error code, e.g. `not_found: bucket not found: Reading` or `invalid_url: ...`, so a client can tell "fix the argument" from "something broke".

Buckets can be named by ULID or by case-insensitive name everywhere a tool takes a `bucket`; `inbox` (or no bucket) means the Inbox.

## Resources

- `stow://item/{id}` — the item rendered as Markdown (what `get_item` returns, readable).
- `stow://bucket/{id}` — a bucket as Markdown: description, notes, tasks, and its items with their `stow://item/…` links.

`resources/list` lists the buckets; both URI shapes are advertised as resource templates.

## Read-only mode

`stw mcp --read-only`, or `STOW_MCP_READ_ONLY=1` (also `true`/`yes`), hides every tool that writes (`add_links`, `move_item`, `archive_item`, `delete_item`, `update_notes`, `add_task`, `complete_task`, `add_reminder`, `create_bucket`, `export`) from `tools/list` and refuses calls to them as unknown tools. Search, listing, `get_item`, `doctor`, and the resources keep working. Use it for assistants you want to let read your library but not change it.

## Data and privacy

- Everything stays on your machine: the server talks to the client over stdio only and opens the same local SQLite database and snapshot files as the app (WAL mode; the app, `stw`, and the MCP server can be open at once).
- The only network access is `add_links` with `fetch` on, which downloads the pages you asked to save, exactly like `stw add`. `localhost` and private-network URLs are saved but not fetched unless `stw` runs with `--allow-local-urls` / `STOW_ALLOW_LOCAL_URLS=true`.
- The client (the assistant) sees whatever the tools return: titles, notes, page text, transcripts, AI notes. Use `--read-only` when it should not change anything.
- API keys are never exposed; the server does not read the keychain.
- stdout is the protocol channel. Logs go to stderr via `tracing`; set `RUST_LOG=stow_mcp=debug` (or `rmcp=debug`) to see them in the client's MCP log.

## Troubleshooting

- "database schema version is newer than this build": the app updated the database; update `stw` too.
- The client reports the server exited immediately: run `echo | stw mcp` in a terminal; it should exit 0 silently. Any error is printed to stderr.
- `stw doctor` (or the `doctor` tool) checks the database and search index; `stw search --rebuild-index` repairs a drifted index.
