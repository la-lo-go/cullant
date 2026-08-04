pub mod ingest;

use std::collections::HashMap;
#[cfg(test)]
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db::Db;
use crate::decode::{classify, FileKind};
use crate::error::AppResult;
use crate::store::{ProjectStore, StoreEntry};

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
    /// Photos whose rating/flag/label came from an XMP sidecar this pass.
    /// Reported once per scan rather than prompting per file — the auto-rescan
    /// interval would make a prompt unbearable.
    pub xmp_imported: usize,
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

/// Turn the backend's flat file listing into classified [`FoundFile`]s,
/// deriving ext/basename/dir from the `/`-separated rel_path (so it works the
/// same for a real filesystem and a SAF tree). Non-media and extension-less
/// files are dropped, matching the previous `WalkDir`-based behavior.
fn collect_found(entries: Vec<StoreEntry>) -> Vec<FoundFile> {
    let mut found = Vec::new();
    for entry in entries {
        let last = entry.rel_path.rsplit('/').next().unwrap_or(&entry.rel_path);
        // Extension = text after the last '.' in the final segment, but a
        // leading-dot-only name (".gitignore") has no extension.
        let ext = match last.rfind('.') {
            Some(i) if i > 0 => last[i + 1..].to_lowercase(),
            _ => continue,
        };
        let Some(kind) = classify(&ext) else {
            continue;
        };
        let basename = match last.rfind('.') {
            Some(i) if i > 0 => last[..i].to_lowercase(),
            _ => last.to_lowercase(),
        };
        let dir = match entry.rel_path.rfind('/') {
            Some(i) => entry.rel_path[..i].to_string(),
            None => String::new(),
        };

        found.push(FoundFile {
            rel_path: entry.rel_path,
            basename,
            dir,
            ext,
            kind,
            size: entry.size,
            mtime: entry.mtime,
        });
    }
    found
}

/// Scan the project folder and reconcile with the database:
/// insert new files (each in a fresh singleton group), update changed ones,
/// mark vanished ones missing, then pair RAW+image files that share
/// dir+basename — but only ever merge *singleton* groups, so existing pairs
/// (including decoupled ones) are never silently rebuilt.
pub fn scan_project(
    app: &AppHandle,
    db: &Arc<Db>,
    store: &dyn ProjectStore,
) -> AppResult<ScanDone> {
    let app_progress = app.clone();
    let done = scan_with_store(db, store, &mut move |found| {
        let _ = app_progress.emit("scan:progress", ScanProgress { found });
    })?;
    let _ = app.emit("scan:done", done.clone());
    Ok(done)
}

/// Test/back-compat helper: scan a real-filesystem project root.
#[cfg(test)]
pub fn scan_project_inner(
    db: &Arc<Db>,
    root: &Path,
    progress: &mut dyn FnMut(usize),
) -> AppResult<ScanDone> {
    let store = crate::store::LocalFsStore::new(root);
    scan_with_store(db, &store, progress)
}

/// Core scan, over any [`ProjectStore`] backend and separated from Tauri event
/// emission so tests can drive it.
pub fn scan_with_store(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    progress: &mut dyn FnMut(usize),
) -> AppResult<ScanDone> {
    let started = std::time::Instant::now();
    // Reported from inside the walk, not after it: classifying the listing is
    // pure in-memory work, so the walk is the only part worth a progress count.
    let entries = store.list_recursive(SKIP_DIRS, progress)?;
    let found = collect_found(entries);
    let total_found = found.len();
    progress(total_found);

    // Captured before the scan transaction takes ownership of `found`. Reusing
    // the listing it already did means the import pass stats nothing itself.
    let sidecar_mtimes: HashMap<String, i64> = found
        .iter()
        .filter(|f| f.kind == FileKind::Sidecar)
        .map(|f| (f.rel_path.clone(), f.mtime))
        .collect();

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

    let xmp_imported = import_sidecars(db, store, &sidecar_mtimes)?;

    tracing::info!(
        "scan finished: {total_found} on disk, {new_files} new, {missing_files} missing, \
         {xmp_imported} imported from XMP, took {:?}",
        started.elapsed()
    );
    Ok(ScanDone {
        file_count,
        new_files,
        missing_files,
        xmp_imported,
    })
}

/// A photo whose sidecar has not been read at its current mtime.
struct SidecarCandidate {
    file_id: i64,
    sc_rel: String,
    sc_mtime: i64,
    /// When this photo's state was last changed inside Cullant; NULL/0 means
    /// never, which is the fresh-import case.
    state_updated_at: i64,
}

/// Pull rating/flag/label/orientation out of XMP sidecars into the database.
///
/// Without this a folder already rated in Lightroom, Bridge or FastRawViewer
/// opens completely blank, which is the commonest way a real library arrives.
///
/// Sidecar mtimes come from the listing the scan already did, so nothing is
/// stat-ed twice, and `files.xmp_source_mtime` short-circuits sidecars that
/// have not changed since they were last read — the steady-state cost of this
/// pass is one query.
fn import_sidecars(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    sidecar_mtimes: &HashMap<String, i64>,
) -> AppResult<usize> {
    if sidecar_mtimes.is_empty() {
        return Ok(0);
    }

    // Every photo that maps to a sidecar we have, paired with what we know.
    // Both halves of a RAW+JPEG pair map to the same sidecar, exactly as the
    // writer does — so exporting and re-importing round-trips instead of
    // silently applying to only one member. A half that has a sidecar of its own
    // reads from that one instead: the writer gives a member its own file only
    // when the pair disagrees, and reading the shared one here would import the
    // partner's state over the difference the user made on purpose.
    let photos: Vec<(i64, String, i64, Option<i64>)> = db.call_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, rel_path, COALESCE(state_updated_at, 0), xmp_source_mtime
             FROM files WHERE status = 0 AND kind IN (0, 1)",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;

    let mut todo: Vec<SidecarCandidate> = Vec::new();
    for (file_id, rel_path, state_updated_at, imported_from) in photos {
        let own = crate::engine::xmp::sidecar_rel_per_file(&rel_path);
        let sc_rel = if sidecar_mtimes.contains_key(own.as_str()) {
            own
        } else {
            crate::engine::xmp::sidecar_rel(&rel_path)
        };
        let Some(&sc_mtime) = sidecar_mtimes.get(sc_rel.as_str()) else {
            continue;
        };
        if imported_from == Some(sc_mtime) {
            continue; // this exact version of the sidecar has been read already
        }
        todo.push(SidecarCandidate {
            file_id,
            sc_rel,
            sc_mtime,
            state_updated_at,
        });
    }
    if todo.is_empty() {
        return Ok(0);
    }

    // Parse each distinct sidecar once, even when a pair shares it.
    let mut parsed: HashMap<String, Option<crate::engine::xmp::XmpState>> = HashMap::new();
    let mut updates: Vec<(i64, i64, Option<crate::engine::xmp::XmpState>)> = Vec::new();
    for c in todo {
        let state = parsed.entry(c.sc_rel.clone()).or_insert_with(|| {
            crate::store::read_all(store, &c.sc_rel)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .as_deref()
                .and_then(crate::engine::xmp::read_sidecar)
        });
        let Some(state) = state else {
            // Unreadable or not XMP we understand: remember the mtime anyway so
            // it is not re-parsed every scan, but change nothing.
            updates.push((c.file_id, c.sc_mtime, None));
            continue;
        };
        // Newer wins. A photo never touched in Cullant has no state to defend,
        // so the sidecar always wins the first import — the onboarding case,
        // which needs no prompt.
        let sidecar_wins = c.state_updated_at == 0 || c.sc_mtime > c.state_updated_at;
        updates.push((c.file_id, c.sc_mtime, sidecar_wins.then(|| state.clone())));
    }

    let applied = updates.iter().filter(|(_, _, s)| s.is_some()).count();
    db.call(move |conn| {
        let tx = conn.transaction()?;
        {
            let mut mark = tx.prepare("UPDATE files SET xmp_source_mtime = ?2 WHERE id = ?1")?;
            // Importing must NOT set xmp_dirty: marking a file dirty for state
            // that came out of its own sidecar would queue a write-back of what
            // was just read, and light the commit button on every scan forever.
            let mut apply = tx.prepare(
                "UPDATE files SET rating = ?2, flag = ?3, label = ?4, orientation = ?5,
                        xmp_source_mtime = ?6
                 WHERE id = ?1",
            )?;
            for (file_id, sc_mtime, state) in &updates {
                match state {
                    Some(s) => apply.execute(params![
                        file_id,
                        s.rating,
                        s.flag,
                        s.label,
                        s.orientation,
                        sc_mtime
                    ])?,
                    None => mark.execute(params![file_id, sc_mtime])?,
                };
            }
        }
        tx.commit()?;
        Ok(())
    })?;

    Ok(applied)
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

    /// A Lightroom-style sidecar with a rating and a pick flag.
    fn write_sidecar_file(root: &Path, rel: &str, rating: i64) {
        fs::write(
            root.join(rel),
            format!(
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:xmpDM="http://ns.adobe.com/xmp/1.0/DynamicMedia/"
    xmp:Rating="{rating}" xmp:Label="Green" xmpDM:pick="1"/>
 </rdf:RDF>
</x:xmpmeta>"#
            ),
        )
        .unwrap();
    }

    fn state_of(db: &Arc<Db>, rel: &str) -> (i64, i64, Option<String>) {
        let rel = rel.to_string();
        db.call(move |c| {
            Ok(c.query_row(
                "SELECT rating, flag, label FROM files WHERE rel_path = ?1",
                params![rel],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap()
    }

    fn dirty_count(db: &Arc<Db>) -> i64 {
        db.call(|c| {
            Ok(
                c.query_row("SELECT COUNT(*) FROM files WHERE xmp_dirty = 1", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap()
    }

    #[test]
    fn imports_ratings_from_existing_sidecars_without_queueing_a_writeback() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "IMG_1.cr3");
        write_sidecar_file(root, "IMG_1.xmp", 4);
        let db = Arc::new(Db::open(root).unwrap());

        let done = scan(&db, root);
        assert_eq!(done.xmp_imported, 1);
        assert_eq!(state_of(&db, "IMG_1.cr3"), (4, 1, Some("Green".into())));

        // THE trap: importing must not mark the file dirty, or the commit button
        // would light up on every scan offering to write back what it just read.
        assert_eq!(dirty_count(&db), 0, "import must not queue an XMP write");

        // A second scan re-reads nothing: the sidecar's mtime is unchanged.
        let again = scan(&db, root);
        assert_eq!(again.xmp_imported, 0);
        assert_eq!(dirty_count(&db), 0);
    }

    #[test]
    fn a_pair_takes_its_shared_sidecar_on_both_halves() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "IMG_2.cr3");
        touch(root, "IMG_2.jpg");
        write_sidecar_file(root, "IMG_2.xmp", 5);
        let db = Arc::new(Db::open(root).unwrap());

        let done = scan(&db, root);
        // Both members map to IMG_2.xmp, exactly as the writer treats them, so
        // exporting and re-importing round-trips instead of desyncing the pair.
        assert_eq!(done.xmp_imported, 2);
        assert_eq!(state_of(&db, "IMG_2.cr3").0, 5);
        assert_eq!(state_of(&db, "IMG_2.jpg").0, 5);
    }

    #[test]
    fn a_half_with_its_own_sidecar_reads_that_one_not_the_shared_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "IMG_7.cr3");
        touch(root, "IMG_7.jpg");
        // What a committed divergence leaves on disk: the primary keeps the
        // basename sidecar, the other half has one of its own.
        write_sidecar_file(root, "IMG_7.xmp", 5);
        write_sidecar_file(root, "IMG_7.jpg.xmp", 1);
        let db = Arc::new(Db::open(root).unwrap());

        scan(&db, root);

        // Reading the shared sidecar for the JPEG too would quietly undo the
        // difference the user made on purpose.
        assert_eq!(state_of(&db, "IMG_7.cr3").0, 5);
        assert_eq!(state_of(&db, "IMG_7.jpg").0, 1);
    }

    #[test]
    fn local_state_newer_than_the_sidecar_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "IMG_3.cr3");
        write_sidecar_file(root, "IMG_3.xmp", 2);
        let db = Arc::new(Db::open(root).unwrap());
        scan(&db, root);
        assert_eq!(state_of(&db, "IMG_3.cr3").0, 2);

        // Cull it in Cullant, stamped later than the sidecar's mtime, then
        // pretend the sidecar changed so it is reconsidered at all.
        db.call(|c| {
            c.execute(
                "UPDATE files SET rating = 5, state_updated_at = strftime('%s','now') + 3600,
                        xmp_source_mtime = NULL",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let done = scan(&db, root);
        assert_eq!(done.xmp_imported, 0, "the older sidecar must not win");
        assert_eq!(state_of(&db, "IMG_3.cr3").0, 5, "our newer rating stands");
    }

    #[test]
    fn an_unreadable_sidecar_changes_nothing_and_is_not_reparsed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "IMG_4.cr3");
        fs::write(root.join("IMG_4.xmp"), b"this is not xmp").unwrap();
        let db = Arc::new(Db::open(root).unwrap());

        let done = scan(&db, root);
        assert_eq!(done.xmp_imported, 0);
        assert_eq!(state_of(&db, "IMG_4.cr3"), (0, 0, None), "nothing wiped");
        // Its mtime is still recorded, so it is not parsed again every scan.
        let marked: i64 = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM files WHERE xmp_source_mtime IS NOT NULL",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(marked, 1);
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
