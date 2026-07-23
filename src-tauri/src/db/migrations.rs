use rusqlite::Connection;

use crate::error::AppResult;

/// Ordered list of schema migrations. `schema_meta.version` records the last
/// applied index + 1, so appending a new SQL block here is all a migration takes.
const MIGRATIONS: &[&str] = &[
    // v1 — initial schema
    r#"
    CREATE TABLE schema_meta (version INTEGER NOT NULL);

    CREATE TABLE project (
      id INTEGER PRIMARY KEY CHECK (id = 1),
      root_path TEXT NOT NULL,
      created_at INTEGER NOT NULL,
      settings TEXT NOT NULL DEFAULT '{}'
    );

    -- One row per physical file. Culling state lives HERE (per-file);
    -- mirror mode fans writes out to group members.
    CREATE TABLE files (
      id INTEGER PRIMARY KEY,
      rel_path TEXT NOT NULL UNIQUE,
      basename TEXT NOT NULL,
      dir TEXT NOT NULL,
      ext TEXT NOT NULL,
      kind INTEGER NOT NULL,              -- 0=raw 1=image 2=video 3=sidecar
      size INTEGER NOT NULL,
      mtime INTEGER NOT NULL,
      quick_hash TEXT,
      capture_time INTEGER,
      width INTEGER, height INTEGER,
      camera TEXT, lens TEXT, iso INTEGER, orientation INTEGER,
      duration_ms INTEGER,
      status INTEGER NOT NULL DEFAULT 0,  -- 0=present 1=missing 2=deleted
      group_id INTEGER NOT NULL REFERENCES groups(id),
      rating INTEGER NOT NULL DEFAULT 0 CHECK (rating BETWEEN 0 AND 5),
      flag INTEGER NOT NULL DEFAULT 0,    -- -1 reject, 0 unflagged, 1 pick
      label TEXT,
      state_updated_at INTEGER,
      xmp_dirty INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_files_group ON files(group_id);
    CREATE INDEX idx_files_pair  ON files(dir, basename);
    CREATE INDEX idx_files_cull  ON files(kind, flag, rating, label);
    CREATE INDEX idx_files_time  ON files(capture_time);

    -- Logical photo. Every file belongs to a group (singleton when unpaired).
    CREATE TABLE groups (
      id INTEGER PRIMARY KEY,
      primary_file_id INTEGER,
      decoupled INTEGER NOT NULL DEFAULT 0,
      created_at INTEGER NOT NULL
    );

    CREATE TABLE task_tags (
      id INTEGER PRIMARY KEY,
      name TEXT NOT NULL UNIQUE,
      shortcut TEXT,
      scope INTEGER NOT NULL DEFAULT 2,   -- 0=photo 1=video 2=both
      color TEXT,
      sort_order INTEGER NOT NULL DEFAULT 0,
      builtin INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE file_tags (
      file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
      tag_id  INTEGER NOT NULL REFERENCES task_tags(id) ON DELETE CASCADE,
      PRIMARY KEY (file_id, tag_id)
    );

    -- Deferred actions queue, per file. A mirrored "delete pair" enqueues
    -- two rows sharing pair_token.
    CREATE TABLE pending_actions (
      id INTEGER PRIMARY KEY,
      file_id INTEGER NOT NULL REFERENCES files(id),
      action INTEGER NOT NULL,            -- 0=delete 1=move 2=copy 3=write_xmp
      params TEXT NOT NULL DEFAULT '{}',
      pair_token TEXT,
      origin INTEGER NOT NULL,            -- 0=manual 1=rule
      created_at INTEGER NOT NULL,
      UNIQUE (file_id, action)
    );

    CREATE TABLE commits (
      id INTEGER PRIMARY KEY,
      started_at INTEGER NOT NULL,
      finished_at INTEGER,
      status INTEGER NOT NULL,            -- 0=running 1=done 2=partial 3=failed
      summary TEXT NOT NULL
    );
    CREATE TABLE commit_entries (
      id INTEGER PRIMARY KEY,
      commit_id INTEGER NOT NULL REFERENCES commits(id),
      file_id INTEGER,
      action INTEGER NOT NULL,
      before_path TEXT, after_path TEXT,
      undo_info TEXT,
      result INTEGER NOT NULL,            -- 0=ok 1=skipped 2=error
      error TEXT
    );

    CREATE TABLE thumbnails (
      file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
      kind INTEGER NOT NULL,              -- 0=thumb 1=preview 2=poster
      cache_path TEXT NOT NULL,
      width INTEGER, height INTEGER,
      source_mtime INTEGER NOT NULL,
      generated_at INTEGER NOT NULL,
      PRIMARY KEY (file_id, kind)
    );

    CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);

    -- Reserved for future AI-assisted culling.
    CREATE TABLE file_analysis (
      file_id INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
      phash BLOB, blur_score REAL, exposure_score REAL,
      embedding BLOB, analyzed_at INTEGER
    );
    CREATE TABLE similarity_clusters (
      cluster_id INTEGER NOT NULL,
      file_id INTEGER NOT NULL REFERENCES files(id),
      score REAL,
      PRIMARY KEY (cluster_id, file_id)
    );

    INSERT INTO schema_meta (version) VALUES (1);
    "#,
    // v2 — SAF document-id cache (Android only; empty/unused on desktop).
    // Maps a project-root-relative path to its opaque SAF document id so the
    // Android storage backend can address files without re-walking the tree.
    r#"
    CREATE TABLE saf_documents (
      rel_path TEXT PRIMARY KEY,
      document_id TEXT NOT NULL,
      is_dir INTEGER NOT NULL DEFAULT 0
    );
    "#,
    // v3 — decode-failure tombstones. A thumbnails row with failed = 1 records
    // that a source could not be decoded at its current mtime, so the ingest
    // pass and the on-demand thumb worker stop retrying it until the file
    // changes. cache_path is '' for such rows.
    r#"
    ALTER TABLE thumbnails ADD COLUMN failed INTEGER NOT NULL DEFAULT 0;
    "#,
    // v4 — per-project UI session state. A single-row table holding one opaque
    // JSON blob (last sort field/direction, media tab, active filters, focused
    // item) so a project reopens where the user left off. The shape is owned by
    // the frontend; the backend stores and returns the blob verbatim.
    r#"
    CREATE TABLE session_state (
      id INTEGER PRIMARY KEY CHECK (id = 1),
      state TEXT NOT NULL DEFAULT '{}'
    );
    "#,
    // v5 — photographic-settings columns for metadata filtering. focal_length is
    // in millimetres, f_number is the aperture value (e.g. 2.8). Both are read
    // during the scan's EXIF pass alongside camera/lens/iso. Existing photos
    // already have capture_time set — that column is the gate the pass keys on —
    // so NULL it for photos to force one more pass on the next open, backfilling
    // the two new columns (and refreshing camera/lens/iso). capture_time is
    // re-derived from EXIF or falls back to mtime, so nothing is lost; videos are
    // left untouched.
    r#"
    ALTER TABLE files ADD COLUMN focal_length REAL;
    ALTER TABLE files ADD COLUMN f_number REAL;
    UPDATE files SET capture_time = NULL WHERE kind IN (0, 1);
    "#,
    // v6 — shutter speed (exposure_time, in seconds) for the metadata filter,
    // read in the same EXIF pass as the v5 columns. Same one-time backfill trick:
    // NULL the photos' capture_time so the next open re-extracts once and fills
    // the column (re-derived or mtime fallback, nothing lost; videos untouched).
    r#"
    ALTER TABLE files ADD COLUMN exposure_time REAL;
    UPDATE files SET capture_time = NULL WHERE kind IN (0, 1);
    "#,
];

pub fn run(conn: &mut Connection) -> AppResult<()> {
    let current: i64 = conn
        .query_row("SELECT version FROM schema_meta", [], |row| row.get(0))
        .unwrap_or(0);

    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let target = (i + 1) as i64;
        if target <= current {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute("UPDATE schema_meta SET version = ?1", [target])?;
        tx.commit()?;
        tracing::info!("applied db migration v{target}");
    }
    Ok(())
}

pub fn current_version(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT version FROM schema_meta", [], |row| row.get(0))?)
}
