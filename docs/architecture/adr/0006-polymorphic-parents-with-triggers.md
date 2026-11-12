# ADR-0006: Polymorphic parent columns with delete-cascade triggers, not foreign keys

- Status: Accepted (revised 2026-11-08 to match what shipped in migration 0002)
- Date: 2026-11-08

## Context
Tasks, attachments, recordings, and AI runs can hang off either a bucket or an item. Reminders can target a bucket, an item, or a task. The schema models this with a pair of columns, `parent_type`/`parent_id` (or `target_type`/`target_id`), plus a `CHECK` on the type column. SQLite foreign keys can reference only one table, so a single `REFERENCES` clause cannot express "points at buckets or items depending on `parent_type`".

Without any database-level guard, deleting a parent from any writer (the app, the `stw` CLI, a hand edit with `sqlite3`) could leave orphans: a task whose item is gone, a reminder for a deleted task, a search document for a vanished recording. Before 0002 each repo delete function issued its own list of child `DELETE`s, which had to be kept identical in two places (items and buckets) and was invisible to other writers.

## Decision
- Keep the polymorphic column pair. Do not split child tables per parent type, and do not add nullable `bucket_id`/`item_id`/`task_id` columns with a one-of constraint.
- Cascade deletes with **`AFTER DELETE` triggers** defined in migration `0002_v2.sql`: `items_cascade`, `buckets_cascade`, `tasks_cascade`, `recordings_cascade`, `ai_runs_cascade`. Each deletes the dependent rows in every child table (tasks, reminders, attachments, recordings, ai_runs) and the matching `search_docs` rows; the task and recording triggers run in turn for the rows the item trigger removes. `buckets_cascade` also nulls `bucket_id` on the remaining search docs, mirroring the real FK `items.bucket_id … ON DELETE SET NULL`.
- **No insert-time guards.** Earlier drafts of this ADR proposed `BEFORE INSERT` triggers that `RAISE(ABORT)` when the referenced parent does not exist. They were not shipped: every command validates ids as ULIDs, `items::create`/`move_to` check the bucket exists (`require_bucket`), and child rows are only ever created from a UI or CLI that already holds the parent. A generic trigger abort would give users a worse error than the `not_found` / `invalid_input` they get now, and the delete cascades already clean up anything that slips through.
- File cleanup (snapshots, attachments, audio, assets) stays in Rust. `items::delete` and `buckets::delete` collect the files owned by the rows about to disappear (`OwnedFiles::collect`) inside the transaction, delete the parent, commit, and only then remove the files. Triggers cannot touch the filesystem.
- Search-document *membership* stays in Rust too (`search::index_conn` on every mutating repo function), because snapshot text, transcripts, and AI output arrive asynchronously and need the owning bucket attached. Only the FTS *index* over `search_docs` is trigger-maintained (`search_docs_ai/ad/au`), as an external-content FTS5 table requires.

## Consequences
- Row integrity on delete holds for every writer: the app, the CLI, and anyone editing `stow.db` with `sqlite3`.
- Queries by parent stay simple (`WHERE parent_type = ? AND parent_id = ?`, backed by the composite indexes).
- Triggers live in migrations, so changing them means a new migration that drops and recreates them. Adding a new child table means extending the parent triggers in the same migration and adding its files to `OwnedFiles`.
- Row cascades in triggers and file cleanup in Rust are two places to keep aligned. The delete tests in `repo/items.rs` and `repo/buckets.rs` check both; `migration_chain_applies_from_empty_and_from_v1` checks that a fresh database and an upgraded one have the same triggers.
- A dangling child created by a buggy writer is not rejected at insert time; it is simply unreachable from the UI and removed when its (nonexistent) parent's id is never deleted. This is accepted as the lesser evil versus opaque constraint errors.

## Alternatives considered
- **Separate tables per parent (`bucket_tasks`, `item_tasks`, …).** Real foreign keys with `ON DELETE CASCADE`, but every feature doubles its tables, queries, models, and commands.
- **Nullable `bucket_id` + `item_id` (+ `task_id`) with a `CHECK` that exactly one is set.** Real foreign keys, but three nullable columns per table, awkward indexes, and the `(type, id)` pair is what the IPC contract and the UI already pass around.
- **Insert-time `RAISE(ABORT)` triggers in addition to the cascades.** Considered and dropped for v0.1 (see Decision); can be added in a later migration if a real orphan source appears.
- **Application-level integrity only.** Zero schema complexity, but the CLI, future importers, and hand edits bypass it, and the failure mode is silent orphans.
