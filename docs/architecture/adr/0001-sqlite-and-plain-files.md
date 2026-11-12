# ADR-0001: SQLite as source of truth, plain files for blobs

- Status: Accepted
- Date: 2026-11-08

## Context
Stow stores structured records (buckets, items, tasks, reminders, AI runs) alongside large blobs (HTML snapshots, attachments, WAV audio, 75–500 MB Whisper models). It is single-user and local-first. It needs full-text search across titles, notes, snapshots, and transcripts, and it must survive crashes mid-write.

## Decision
- One SQLite database (`stow.db`, WAL mode, `foreign_keys = ON`) through `rusqlite` with the bundled SQLite, so FTS5 is guaranteed.
- Blobs are ordinary files under the data folder (`snapshots/`, `attachments/`, `audio/`, `models/`, `assets/`). Rows store their paths.
- Schema changes are numbered, forward-only SQL migrations applied at open inside a transaction.
- Search documents live in a plain `search_docs` table that repo functions maintain explicitly, so snapshot text and transcripts can be indexed when they arrive asynchronously; since migration 0002 an external-content FTS5 table indexes it and is kept in step by triggers (see [data-model.md](../data-model.md)).
- IDs are ULIDs (sortable by creation time). Timestamps are RFC 3339 UTC strings.
- A single `Mutex<Connection>` is used, which is enough for one user.

## Consequences
- Users can inspect, back up, and script their data with standard tools. Snapshots are readable Markdown on disk.
- Deletes must cascade to files as well as rows. Repo delete functions own that, and tests cover it.
- The CLI and the app can open the same database concurrently (WAL + `busy_timeout`), but the app only notices CLI changes on reload.
- Sync between machines isn't provided. Export (Markdown/JSON) and the online database backup are the portability story.

## Alternatives considered
- **Markdown files as the source of truth (Obsidian-style).** Nicer for hand editing, but reminders, task state, job status, and dedupe by canonical URL become fragile file parsing.
- **Blobs inside SQLite.** One file to back up, but it bloats the database and WAL, and makes attachments hard to open in other apps.
- **An embedded KV store (sled, redb).** No SQL, no FTS, and fewer tools for users.
