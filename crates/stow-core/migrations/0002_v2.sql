-- v2: done state, retry counters, better indexes, DB-level cascades for the polymorphic
-- parent/target columns, and an external-content FTS5 index over a plain side table.

ALTER TABLE items ADD COLUMN done_at TEXT;
ALTER TABLE items ADD COLUMN snapshot_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE recordings ADD COLUMN transcript_attempts INTEGER NOT NULL DEFAULT 0;

DROP INDEX IF EXISTS items_bucket_position;
CREATE INDEX items_bucket_created ON items(bucket_id, created_at DESC);
CREATE INDEX items_snapshot_status ON items(snapshot_status);
CREATE INDEX reminders_unfired_effective ON reminders(COALESCE(snoozed_until, fire_at))
  WHERE fired_at IS NULL;

-- Search: rows live in search_docs; `search` indexes title/body by rowid.
CREATE TABLE search_docs (
  id INTEGER PRIMARY KEY,
  doc_type TEXT NOT NULL,
  doc_id TEXT NOT NULL,
  item_id TEXT,
  bucket_id TEXT,
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  UNIQUE(doc_type, doc_id)
);
CREATE INDEX search_docs_item ON search_docs(item_id);
CREATE INDEX search_docs_bucket ON search_docs(bucket_id);

INSERT OR REPLACE INTO search_docs (doc_type, doc_id, item_id, bucket_id, title, body)
  SELECT doc_type, doc_id, item_id, bucket_id, COALESCE(title, ''), COALESCE(body, '')
  FROM search ORDER BY rowid;

-- Docs owned by an item (recordings, AI runs) now carry the item's bucket too.
UPDATE search_docs
  SET bucket_id = (SELECT bucket_id FROM items WHERE items.id = search_docs.item_id)
  WHERE item_id IS NOT NULL AND EXISTS (SELECT 1 FROM items WHERE items.id = search_docs.item_id);

DROP TABLE search;
CREATE VIRTUAL TABLE search USING fts5(
  title,
  body,
  content = 'search_docs',
  content_rowid = 'id',
  tokenize = 'porter unicode61'
);

CREATE TRIGGER search_docs_ai AFTER INSERT ON search_docs BEGIN
  INSERT INTO search (rowid, title, body) VALUES (new.id, new.title, new.body);
END;
CREATE TRIGGER search_docs_ad AFTER DELETE ON search_docs BEGIN
  INSERT INTO search (search, rowid, title, body) VALUES ('delete', old.id, old.title, old.body);
END;
CREATE TRIGGER search_docs_au AFTER UPDATE OF title, body ON search_docs BEGIN
  INSERT INTO search (search, rowid, title, body) VALUES ('delete', old.id, old.title, old.body);
  INSERT INTO search (rowid, title, body) VALUES (new.id, new.title, new.body);
END;

INSERT INTO search (search) VALUES ('rebuild');

-- Cascades. Rust still removes the files these rows point at, after the commit.
CREATE TRIGGER tasks_cascade AFTER DELETE ON tasks BEGIN
  DELETE FROM reminders WHERE target_type = 'task' AND target_id = old.id;
END;

CREATE TRIGGER recordings_cascade AFTER DELETE ON recordings BEGIN
  DELETE FROM search_docs WHERE doc_type = 'recording' AND doc_id = old.id;
END;

CREATE TRIGGER ai_runs_cascade AFTER DELETE ON ai_runs BEGIN
  DELETE FROM search_docs WHERE doc_type = 'ai_run' AND doc_id = old.id;
END;

CREATE TRIGGER items_cascade AFTER DELETE ON items BEGIN
  DELETE FROM tasks WHERE parent_type = 'item' AND parent_id = old.id;
  DELETE FROM attachments WHERE parent_type = 'item' AND parent_id = old.id;
  DELETE FROM recordings WHERE parent_type = 'item' AND parent_id = old.id;
  DELETE FROM ai_runs WHERE parent_type = 'item' AND parent_id = old.id;
  DELETE FROM reminders WHERE target_type = 'item' AND target_id = old.id;
  DELETE FROM search_docs WHERE doc_type IN ('item', 'snapshot') AND doc_id = old.id;
END;

CREATE TRIGGER buckets_cascade AFTER DELETE ON buckets BEGIN
  DELETE FROM tasks WHERE parent_type = 'bucket' AND parent_id = old.id;
  DELETE FROM attachments WHERE parent_type = 'bucket' AND parent_id = old.id;
  DELETE FROM recordings WHERE parent_type = 'bucket' AND parent_id = old.id;
  DELETE FROM ai_runs WHERE parent_type = 'bucket' AND parent_id = old.id;
  DELETE FROM reminders WHERE target_type = 'bucket' AND target_id = old.id;
  DELETE FROM search_docs WHERE doc_type = 'bucket' AND doc_id = old.id;
  -- The bucket's items fall back to the Inbox (FK ON DELETE SET NULL); so do their docs.
  UPDATE search_docs SET bucket_id = NULL WHERE bucket_id = old.id;
END;
