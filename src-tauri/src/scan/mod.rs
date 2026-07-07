pub mod metadata;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

use crate::db::Db;
use crate::decode::{classify, FileKind};
use crate::error::AppResult;

#[derive(Debug)]
struct FoundFile {
    rel_path: String,
    basename: String,
    dir: String,
    ext: String,
    kind: FileKind,
    size: i64,
    mtime: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub found: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanDone {
    pub file_count: i64,
    pub new_files: usize,
    pub missing_files: usize,
}

const SKIP_DIRS: &[&str] = &[
    ".cullant",
    "_trash",
    "$RECYCLE.BIN",
    "System Volume Information",
];

fn unix_secs(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn walk(root: &Path, progress: &mut dyn FnMut(usize)) -> Vec<FoundFile> {
    let mut found = Vec::new();
    let entries = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true; // never filter the project root itself
            }
            let name = e.file_name().to_string_lossy();
            !(e.file_type().is_dir()
                && (SKIP_DIRS.contains(&name.as_ref()) || name.starts_with('.')))
        });

    for entry in entries.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(ext) = path.extension().map(|e| e.to_string_lossy().to_lowercase()) else {
            continue;
        };
        let Some(kind) = classify(&ext) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel_path = rel.to_string_lossy().replace('\\', "/");
        let basename = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let dir = match rel_path.rfind('/') {
            Some(i) => rel_path[..i].to_string(),
            None => String::new(),
        };

        found.push(FoundFile {
            rel_path,
            basename,
            dir,
            ext,
            kind,
            size: meta.len() as i64,
            mtime: meta.modified().map(unix_secs).unwrap_or(0),
        });

        if found.len() % 500 == 0 {
            progress(found.len());
        }
    }
    found
}

/// Scan the project folder and reconcile with the database:
/// insert new files (each in a fresh singleton group), update changed ones,
/// mark vanished ones missing, then pair RAW+image files that share
/// dir+basename — but only ever merge *singleton* groups, so existing pairs
/// (including decoupled ones) are never silently rebuilt.
pub fn scan_project(app: &AppHandle, db: &Arc<Db>, root: &Path) -> AppResult<ScanDone> {
    let app_progress = app.clone();
    let done = scan_project_inner(db, root, &mut move |found| {
        let _ = app_progress.emit("scan:progress", ScanProgress { found });
    })?;
    let _ = app.emit("scan:done", done.clone());
    Ok(done)
}

/// Core scan, separated from Tauri event emission so tests can drive it.
pub fn scan_project_inner(
    db: &Arc<Db>,
    root: &Path,
    progress: &mut dyn FnMut(usize),
) -> AppResult<ScanDone> {
    let started = std::time::Instant::now();
    let found = walk(root, progress);
    let total_found = found.len();

    let (new_files, missing_files, file_count) = db.call(move |conn| {
        let now_secs = unix_secs(SystemTime::now());
        let tx = conn.transaction()?;
        let mut new_files = 0usize;

        {
            // Existing state: rel_path -> (id, size, mtime, status)
            let mut existing: HashMap<String, (i64, i64, i64, i64)> = HashMap::new();
            let mut stmt = tx.prepare("SELECT rel_path, id, size, mtime, status FROM files")?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?),
                ))
            })?;
            for row in rows {
                let (rel, tuple) = row?;
                existing.insert(rel, tuple);
            }

            let mut insert_group = tx.prepare("INSERT INTO groups (created_at) VALUES (?1)")?;
            let mut insert_file = tx.prepare(
                "INSERT INTO files (rel_path, basename, dir, ext, kind, size, mtime, group_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            let mut update_file = tx.prepare(
                "UPDATE files SET size = ?2, mtime = ?3, status = 0, capture_time = NULL
                 WHERE id = ?1",
            )?;
            let mut revive_file = tx.prepare("UPDATE files SET status = 0 WHERE id = ?1")?;
            let mut set_primary =
                tx.prepare("UPDATE groups SET primary_file_id = ?2 WHERE id = ?1")?;

            for f in &found {
                match existing.remove(&f.rel_path) {
                    None => {
                        insert_group.execute(params![now_secs])?;
                        let group_id = tx.last_insert_rowid();
                        insert_file.execute(params![
                            f.rel_path,
                            f.basename,
                            f.dir,
                            f.ext,
                            f.kind as i64,
                            f.size,
                            f.mtime,
                            group_id
                        ])?;
                        set_primary.execute(params![group_id, tx.last_insert_rowid()])?;
                        new_files += 1;
                    }
                    Some((id, size, mtime, status)) => {
                        if size != f.size || mtime != f.mtime {
                            // Content changed: refresh stats, invalidate metadata.
                            update_file.execute(params![id, f.size, f.mtime])?;
                        } else if status != 0 {
                            revive_file.execute(params![id])?;
                        }
                    }
                }
            }

            // Anything left in `existing` was not found on disk.
            let mut mark_missing = tx.prepare("UPDATE files SET status = 1 WHERE id = ?1")?;
            for (id, _, _, status) in existing.values() {
                if *status == 0 {
                    mark_missing.execute(params![id])?;
                }
            }
        }

        // Pairing pass: merge singleton raw+image groups sharing dir+basename.
        let pairs: Vec<(i64, i64, i64, i64)> = {
            let mut stmt = tx.prepare(
                "SELECT raw.id, raw.group_id, img.id, img.group_id
                 FROM files raw
                 JOIN files img ON img.dir = raw.dir AND img.basename = raw.basename
                 WHERE raw.kind = 0 AND img.kind = 1
                   AND raw.status = 0 AND img.status = 0
                   AND raw.group_id <> img.group_id
                   AND (SELECT COUNT(*) FROM files m WHERE m.group_id = raw.group_id) = 1
                   AND (SELECT COUNT(*) FROM files m WHERE m.group_id = img.group_id) = 1",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        for (raw_id, raw_group, img_id, img_group) in pairs {
            tx.execute(
                "UPDATE files SET group_id = ?1 WHERE id = ?2",
                params![raw_group, img_id],
            )?;
            tx.execute("DELETE FROM groups WHERE id = ?1", params![img_group])?;
            tx.execute(
                "UPDATE groups SET primary_file_id = ?2 WHERE id = ?1",
                params![raw_group, raw_id],
            )?;
        }

        let missing: i64 =
            tx.query_row("SELECT COUNT(*) FROM files WHERE status = 1", [], |r| {
                r.get(0)
            })?;
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM files WHERE status = 0", [], |r| {
            r.get(0)
        })?;
        tx.commit()?;
        Ok((new_files, missing as usize, count))
    })?;

    tracing::info!(
        "scan finished: {total_found} on disk, {new_files} new, {missing_files} missing, took {:?}",
        started.elapsed()
    );
    Ok(ScanDone {
        file_count,
        new_files,
        missing_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(root: &Path, rel: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    fn scan(db: &Arc<Db>, root: &Path) -> ScanDone {
        scan_project_inner(db, root, &mut |_| {}).unwrap()
    }

    #[test]
    fn pairs_raw_and_jpeg_by_dir_and_basename() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "shoot/IMG_001.CR3");
        touch(root, "shoot/IMG_001.JPG");
        touch(root, "shoot/IMG_002.JPG");
        touch(root, "other/IMG_001.JPG"); // same basename, different dir: no pair
        let db = Arc::new(Db::open(root).unwrap());

        let done = scan(&db, root);
        assert_eq!(done.file_count, 4);
        assert_eq!(done.new_files, 4);

        let group_count: i64 = db
            .call(|c| Ok(c.query_row("SELECT COUNT(*) FROM groups", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(group_count, 3); // pair + 2 singletons

        let pair_size: i64 = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM files WHERE group_id =
                     (SELECT group_id FROM files WHERE rel_path = 'shoot/IMG_001.CR3')",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(pair_size, 2);
    }

    #[test]
    fn rescan_is_stable_and_detects_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "a.jpg");
        touch(root, "b.cr3");
        let db = Arc::new(Db::open(root).unwrap());

        let first = scan(&db, root);
        assert_eq!(first.new_files, 2);

        let second = scan(&db, root);
        assert_eq!(second.new_files, 0);
        assert_eq!(second.file_count, 2);

        fs::remove_file(root.join("a.jpg")).unwrap();
        let third = scan(&db, root);
        assert_eq!(third.file_count, 1);
        assert_eq!(third.missing_files, 1);
    }

    #[test]
    fn skips_internal_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "keep.jpg");
        touch(root, "_trash/gone.jpg");
        let db = Arc::new(Db::open(root).unwrap());
        // .cullant dir created by Db::open; drop a decoy inside it too.
        touch(root, ".cullant/decoy.jpg");

        let done = scan(&db, root);
        assert_eq!(done.file_count, 1);
    }
}
