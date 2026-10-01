pub mod ingest;

use std::collections::HashMap;
#[cfg(test)]
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Transaction};
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
    pub project_root: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanDone {
    pub project_root: String,
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
        let Some(ext) = crate::decode::ext_of(&entry.rel_path) else {
            continue;
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
/// mark vanished ones missing, then group the photo files that share
/// dir+basename. A latecomer joins the group its shot already has; a decoupled
/// group is never rebuilt.
pub fn scan_project(
    app: &AppHandle,
    db: &Arc<Db>,
    store: &dyn ProjectStore,
) -> AppResult<ScanDone> {
    let app_progress = app.clone();
    let project_root = db.project_root();
    let done = scan_with_store(db, store, &mut move |found| {
        let _ = app_progress.emit(
            "scan:progress",
            ScanProgress {
                found,
                project_root: project_root.clone(),
            },
        );
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

/// One live photo file in a dir+basename bucket, as the pairing pass sees it.
struct Candidate {
    id: i64,
    group_id: i64,
    ext: String,
    /// Live members already in this file's group. More than one means an
    /// *established* group: an earlier scan built it, or the user shaped it.
    group_size: i64,
    decoupled: bool,
}

/// Merge one dir+basename bucket into a single group, and give that group its
/// best-ranked member as primary.
///
/// The bucket can already hold an established group — the common case, because a
/// RAW+JPEG pair is built on the first scan and the `.HIF` or `.ORI` of the same
/// shot arrives on a later one, or in a later copy. That group is the survivor
/// and the latecomers join it. Merging the latecomers with each other instead
/// would split one shot across two cells, and rejecting the shot would then
/// delete only half of its files.
///
/// Two cases refuse to merge, because both mean the user has shaped the grouping
/// and a scan must not silently undo that: a decoupled group, and a bucket that
/// somehow spans two established groups.
///
/// `members` is ordered by rel_path, so the rank tie break is stable across
/// rescans.
fn merge_group(tx: &Transaction, members: &[Candidate]) -> rusqlite::Result<()> {
    if members.len() < 2 {
        return Ok(());
    }
    // Checked over every member, not only over an established group: a decoupled
    // pair that has lost a half looks like a singleton, and merging a latecomer
    // into it would revive a grouping the user took apart.
    if members.iter().any(|m| m.decoupled) {
        return Ok(());
    }

    let mut established: Vec<i64> = members
        .iter()
        .filter(|m| m.group_size > 1)
        .map(|m| m.group_id)
        .collect();
    established.sort_unstable();
    established.dedup();

    let best = members
        .iter()
        .min_by_key(|m| crate::decode::primary_rank(&m.ext));
    let survivor = match (established.as_slice(), best) {
        ([], Some(m)) => m.group_id,
        ([group_id], _) => *group_id,
        _ => return Ok(()),
    };

    for m in members {
        if m.group_id == survivor {
            continue;
        }
        // Moves the whole group, not just this row. `group_size` counts live
        // members only, so a group can also hold a committed delete (status = 2)
        // — and `files.group_id` is a foreign key with `foreign_keys = ON`, so
        // leaving that row behind and dropping its group would fail the scan
        // transaction, and every scan after it.
        tx.prepare_cached("UPDATE files SET group_id = ?1 WHERE group_id = ?2")?
            .execute(params![survivor, m.group_id])?;
        tx.prepare_cached(
            "DELETE FROM groups WHERE id = ?1
               AND NOT EXISTS (SELECT 1 FROM files WHERE group_id = ?1)",
        )?
        .execute(params![m.group_id])?;
    }

    // Recomputed over the whole membership, not carried over: when a real RAW
    // joins a group a JPEG was representing, the RAW takes the cell.
    if let Some(primary) = best {
        tx.prepare_cached("UPDATE groups SET primary_file_id = ?2 WHERE id = ?1")?
            .execute(params![survivor, primary.id])?;
    }
    Ok(())
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
    let _profile = crate::photo_profile::span("scan");
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
                            update_file.execute(params![id, f.size, f.mtime])?;
                        } else if status != 0 {
                            revive_file.execute(params![id])?;
                        }
                    }
                }
            }

            let mut mark_missing = tx.prepare("UPDATE files SET status = 1 WHERE id = ?1")?;
            for (id, _, _, status) in existing.values() {
                if *status == 0 {
                    mark_missing.execute(params![id])?;
                }
            }
        }

        // Pairing pass: gather the live photo files of each dir+basename that do
        // not yet share one group, and merge them. A shot is not always two files
        // — OM System writes ORF+ORI+JPG in Live ND, and a RAW+HEIF camera adds a
        // .HIF — so the members are collected per basename rather than matched in
        // pairs.
        //
        // The selection asks for a basename spanning more than one group, not for
        // singletons: a latecomer must be able to join the group its shot already
        // has. It also means a settled library selects nothing, so the cost of
        // this pass falls to zero once everything is grouped.
        let candidates: Vec<(String, String, Candidate)> = {
            let mut stmt = tx.prepare(
                "SELECT f.dir, f.basename, f.id, f.group_id, f.ext,
                        (SELECT COUNT(*) FROM files m
                          WHERE m.group_id = f.group_id AND m.status = 0),
                        g.decoupled
                 FROM files f
                 JOIN groups g ON g.id = f.group_id
                 WHERE f.status = 0 AND f.kind IN (0, 1)
                   AND (SELECT COUNT(DISTINCT n.group_id) FROM files n
                          WHERE n.dir = f.dir AND n.basename = f.basename
                            AND n.status = 0 AND n.kind IN (0, 1)) > 1
                 ORDER BY f.dir, f.basename, f.rel_path",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    Candidate {
                        id: r.get(2)?,
                        group_id: r.get(3)?,
                        ext: r.get(4)?,
                        group_size: r.get(5)?,
                        decoupled: r.get::<_, i64>(6)? != 0,
                    },
                ))
            })?;
            rows.collect::<Result<_, _>>()?
        };

        // Equal keys are contiguous because the query orders by directory and basename.
        let mut members: Vec<Candidate> = Vec::new();
        let mut key: Option<(String, String)> = None;
        for (dir, basename, candidate) in candidates {
            let same = key
                .as_ref()
                .is_some_and(|(d, b)| d.as_str() == dir && b.as_str() == basename);
            if !same {
                merge_group(&tx, &members)?;
                members.clear();
                key = Some((dir, basename));
            }
            members.push(candidate);
        }
        merge_group(&tx, &members)?;

        promote_live_primaries(&tx)?;

        let missing: i64 =
            tx.query_row("SELECT COUNT(*) FROM files WHERE status = 1", [], |r| {
                r.get(0)
            })?;
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM files WHERE status = 0 AND kind IN (0, 1, 2)",
            [],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok((new_files, missing as usize, count))
    })?;

    let xmp_imported = import_sidecars(db, store, &sidecar_mtimes)?;
    crate::thumbs::invalidate_cache_rows(db)?;

    tracing::info!(
        "scan finished: {total_found} on disk, {new_files} new, {missing_files} missing, \
         {xmp_imported} imported from XMP, took {:?}",
        started.elapsed()
    );
    Ok(ScanDone {
        project_root: db.project_root(),
        file_count,
        new_files,
        missing_files,
        xmp_imported,
    })
}

/// A photo whose sidecar has not been read at its current mtime.
fn promote_live_primaries(tx: &Transaction) -> rusqlite::Result<()> {
    let members: Vec<(i64, i64, String)> = {
        let mut stmt = tx.prepare(
            "SELECT f.group_id, f.id, f.ext FROM files f JOIN groups g ON g.id = f.group_id
             LEFT JOIN files p ON p.id = g.primary_file_id
             WHERE f.status = 0 AND f.kind IN (0, 1, 2)
               AND (p.id IS NULL OR p.status <> 0 OR p.group_id <> g.id)
             ORDER BY f.group_id, f.rel_path",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut best: HashMap<i64, (i64, u8)> = HashMap::new();
    for (group, id, ext) in members {
        let rank = crate::decode::primary_rank(&ext);
        let entry = best.entry(group).or_insert((id, rank));
        if rank < entry.1 {
            *entry = (id, rank);
        }
    }
    for (group, (id, _)) in best {
        tx.execute(
            "UPDATE groups SET primary_file_id = ?2 WHERE id = ?1",
            params![group, id],
        )?;
    }
    Ok(())
}

struct SidecarCandidate {
    file_id: i64,
    sc_rel: String,
    sc_mtime: i64,
    version: String,
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
    // Every photo that maps to a sidecar we have, paired with what we know.
    // Both halves of a RAW+JPEG pair map to the same sidecar, exactly as the
    // writer does — so exporting and re-importing round-trips instead of
    // silently applying to only one member. A half that has a sidecar of its own
    // reads from that one instead: the writer gives a member its own file only
    // when the pair disagrees, and reading the shared one here would import the
    // partner's state over the difference the user made on purpose.
    let photos: Vec<(i64, String, i64, Option<String>, bool)> = db.call_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, rel_path, COALESCE(state_updated_at, 0), xmp_source_version,
                    xmp_source_mtime IS NOT NULL
             FROM files WHERE status = 0 AND kind IN (0, 1)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;

    let mut todo: Vec<SidecarCandidate> = Vec::new();
    let mut contents: HashMap<String, Option<(String, String)>> = HashMap::new();
    let mut absent = Vec::new();
    for (file_id, rel_path, state_updated_at, imported_from, was_imported) in photos {
        let own = crate::engine::xmp::sidecar_rel_per_file(&rel_path);
        let sc_rel = if sidecar_mtimes.contains_key(own.as_str()) {
            own
        } else {
            crate::engine::xmp::sidecar_rel(&rel_path)
        };
        let Some(&sc_mtime) = sidecar_mtimes.get(sc_rel.as_str()) else {
            if was_imported || imported_from.is_some() {
                absent.push(file_id);
            }
            continue;
        };
        let content = contents.entry(sc_rel.clone()).or_insert_with(|| {
            let bytes = crate::store::read_all(store, &sc_rel).ok()?;
            let version = format!("{sc_mtime}:{:016x}", xxhash_rust::xxh3::xxh3_64(&bytes));
            Some((version, String::from_utf8(bytes).ok()?))
        });
        let Some((version, _)) = content else {
            continue;
        };
        if imported_from.as_ref() == Some(version) {
            continue; // this exact version of the sidecar has been read already
        }
        todo.push(SidecarCandidate {
            file_id,
            sc_rel,
            sc_mtime,
            version: version.clone(),
            state_updated_at,
        });
    }
    if !absent.is_empty() {
        db.call(move |conn| {
            let tx = conn.transaction()?;
            for file_id in absent {
                tx.execute(
                "UPDATE files SET xmp_source_mtime = NULL, xmp_source_version = NULL WHERE id = ?1",
                params![file_id],
                )?;
            }
            tx.commit()?;
            Ok(())
        })?;
    }

    // Parse each distinct sidecar once, even when a pair shares it.
    let mut parsed: HashMap<String, Option<crate::engine::xmp::XmpImport>> = HashMap::new();
    let mut updates = Vec::new();
    for c in todo {
        let state = parsed.entry(c.sc_rel.clone()).or_insert_with(|| {
            contents
                .get(&c.sc_rel)
                .and_then(Option::as_ref)
                .and_then(|(_, xml)| crate::engine::xmp::read_sidecar_import(xml))
        });
        let Some(state) = state else {
            // Unreadable or not XMP we understand: remember the mtime anyway so
            // it is not re-parsed every scan, but change nothing.
            updates.push((c.file_id, c.sc_mtime, c.version, None));
            continue;
        };
        // Newer wins. A photo never touched in Cullant has no state to defend,
        // so the sidecar always wins the first import — the onboarding case,
        // which needs no prompt.
        let sidecar_wins = c.state_updated_at == 0 || c.sc_mtime > c.state_updated_at;
        updates.push((
            c.file_id,
            c.sc_mtime,
            c.version,
            sidecar_wins.then(|| state.clone()),
        ));
    }

    let applied = updates.iter().filter(|(_, _, _, s)| s.is_some()).count();
    db.call(move |conn| {
        let tx = conn.transaction()?;
        {
            let mut mark = tx.prepare("UPDATE files SET xmp_source_mtime = ?2, xmp_source_version = ?3 WHERE id = ?1")?;
            // Importing must NOT set xmp_dirty: marking a file dirty for state
            // that came out of its own sidecar would queue a write-back of what
            // was just read, and light the commit button on every scan forever.
            let mut apply = tx.prepare(
                "UPDATE files SET rating = ?2, flag = ?3, label = ?4, orientation = COALESCE(?5, orientation),
                        xmp_source_mtime = ?6, xmp_source_version = ?7
                 WHERE id = ?1",
            )?;
            for (file_id, sc_mtime, version, state) in &updates {
                match state {
                    Some(s) => apply.execute(params![
                        file_id,
                        s.state.rating,
                        s.state.flag,
                        s.state.label,
                        s.has_orientation.then_some(s.state.orientation),
                        sc_mtime,
                        version
                    ])?,
                    None => mark.execute(params![file_id, sc_mtime, version])?,
                };
                if let Some(import) = state {
                    if import.state.flag == -1 {
                        tx.execute("INSERT INTO pending_actions(file_id, action, params, origin, created_at)
                            VALUES (?1, 0, '{}', 0, ?2) ON CONFLICT(file_id, action) DO NOTHING", params![file_id, sc_mtime])?;
                    } else {
                        tx.execute("DELETE FROM pending_actions WHERE file_id = ?1 AND action = 0", params![file_id])?;
                    }
                }
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

    #[test]
    fn an_unchanged_rescan_without_sidecars_does_not_write_photo_rows() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "x.jpg");
        touch(dir.path(), "y.jpg");
        let db = Arc::new(Db::open(dir.path()).unwrap());
        scan(&db, dir.path());
        let before = db.call(|c| Ok(c.total_changes())).unwrap();
        scan(&db, dir.path());
        let after = db.call(|c| Ok(c.total_changes())).unwrap();
        assert_eq!(before, after, "A no-op scan must not rewrite every photo");
    }

    #[test]
    fn a_missing_primary_is_replaced_by_a_live_member() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "x.orf");
        touch(root, "x.ori");
        touch(root, "x.jpg");
        let db = Arc::new(Db::open(root).unwrap());
        scan(&db, root);
        fs::rename(root.join("x.orf"), root.join("x.saved")).unwrap();
        scan(&db, root);
        assert_eq!(group_of_path(&db, "x.jpg").1, "x.jpg");
    }

    #[test]
    fn sidecars_are_not_counted_as_media() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "x.xmp");
        let db = Arc::new(Db::open(dir.path()).unwrap());
        assert_eq!(scan(&db, dir.path()).file_count, 0);
    }

    #[test]
    fn importing_a_reject_queues_a_delete_and_keeps_existing_orientation() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "x.jpg");
        let db = Arc::new(Db::open(dir.path()).unwrap());
        scan(&db, dir.path());
        db.call(|c| {
            c.execute("UPDATE files SET orientation = 8", [])?;
            Ok(())
        })
        .unwrap();
        fs::write(dir.path().join("x.xmp"), r#"<rdf:Description xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="-1"/>"#).unwrap();
        scan(&db, dir.path());
        assert_eq!(crate::engine::actions::list(&db).unwrap().len(), 1);
        assert_eq!(
            db.call_read(|c| Ok(c.query_row(
                "SELECT orientation FROM files WHERE kind = 1",
                [],
                |r| r.get::<_, i64>(0)
            )?))
            .unwrap(),
            8
        );
    }

    #[test]
    fn a_sidecar_replacement_in_the_same_second_is_imported() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "x.jpg");
        write_sidecar_file(dir.path(), "x.xmp", 2);
        let path = dir.path().join("x.xmp");
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let db = Arc::new(Db::open(dir.path()).unwrap());
        scan(&db, dir.path());
        write_sidecar_file(dir.path(), "x.xmp", 5);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert_eq!(scan(&db, dir.path()).xmp_imported, 1);
        assert_eq!(state_of(&db, "x.jpg").0, 5);
    }

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

    /// The size of the group `rel_path` belongs to, and its group's primary.
    fn group_of_path(db: &Arc<Db>, rel_path: &str) -> (i64, String) {
        let rel = rel_path.to_string();
        db.call(move |c| {
            Ok(c.query_row(
                "SELECT (SELECT COUNT(*) FROM files m WHERE m.group_id = f.group_id),
                        (SELECT p.rel_path FROM files p JOIN groups g ON g.id = f.group_id
                          WHERE p.id = g.primary_file_id)
                 FROM files f WHERE f.rel_path = ?1",
                params![rel],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
    }

    /// OM System writes three files for one Live ND frame. All three are one
    /// photo, and the ORF — not the companion ORI — stands for it.
    #[test]
    fn groups_a_three_file_shot_behind_its_raw() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "P1010001.ORF");
        touch(root, "P1010001.ORI");
        touch(root, "P1010001.JPG");
        let db = Arc::new(Db::open(root).unwrap());

        let done = scan(&db, root);
        assert_eq!(done.file_count, 3);

        let (size, primary) = group_of_path(&db, "P1010001.ORI");
        assert_eq!(size, 3, "the whole shot is one group");
        assert_eq!(primary, "P1010001.ORF");
    }

    /// A RAW+HEIF shot has no JPEG at all. The HEIF must still join the group —
    /// otherwise deleting the rejected shot leaves the .HIF behind — while the
    /// CR3 keeps representing it, since nothing here can decode the HEIF.
    #[test]
    fn a_heif_joins_its_raw_and_never_represents_the_shot() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "IMG_0421.CR3");
        touch(root, "IMG_0421.HIF");
        let db = Arc::new(Db::open(root).unwrap());

        let done = scan(&db, root);
        assert_eq!(done.file_count, 2, "the HIF is catalogued, not skipped");

        let (size, primary) = group_of_path(&db, "IMG_0421.HIF");
        assert_eq!(size, 2);
        assert_eq!(primary, "IMG_0421.CR3");
    }

    /// With no RAW present the decodable JPEG represents the shot, not the HEIF
    /// sitting next to it.
    #[test]
    fn a_jpeg_outranks_a_heif_when_there_is_no_raw() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "IMG_9.JPG");
        touch(root, "IMG_9.HEIC");
        let db = Arc::new(Db::open(root).unwrap());
        scan(&db, root);

        let (size, primary) = group_of_path(&db, "IMG_9.HEIC");
        assert_eq!(size, 2);
        assert_eq!(primary, "IMG_9.JPG");
    }

    fn group_id_of(db: &Arc<Db>, rel_path: &str) -> i64 {
        let rel = rel_path.to_string();
        db.call(move |c| {
            Ok(c.query_row(
                "SELECT group_id FROM files WHERE rel_path = ?1",
                params![rel],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    }

    /// Every existing library already holds RAW+JPEG groups, so a `.HIF` or an
    /// `.ORI` almost always arrives *after* its group exists — on a later scan,
    /// or in a second copy. It must join that group. If it did not, the orphan
    /// this change removes would survive in every library built before it.
    #[test]
    fn a_latecomer_joins_the_group_its_shot_already_has() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "IMG_1.CR3");
        touch(root, "IMG_1.JPG");
        let db = Arc::new(Db::open(root).unwrap());
        scan(&db, root);
        let group_before = group_id_of(&db, "IMG_1.CR3");

        touch(root, "IMG_1.HIF");
        scan(&db, root);

        let (size, primary) = group_of_path(&db, "IMG_1.HIF");
        assert_eq!(size, 3);
        assert_eq!(group_id_of(&db, "IMG_1.HIF"), group_before, "same group");
        assert_eq!(primary, "IMG_1.CR3");
    }

    /// Two latecomers arriving together must not pair with *each other*. A scan
    /// runs on an interval, so it can land between two copies. If the two split
    /// off into a second group, one shot would draw two cells, and rejecting it
    /// would delete only half its files.
    #[test]
    fn two_latecomers_join_the_existing_group_not_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "P101.ORF");
        touch(root, "P101.JPG");
        let db = Arc::new(Db::open(root).unwrap());
        scan(&db, root);
        let group_before = group_id_of(&db, "P101.ORF");

        touch(root, "P101.ORI");
        touch(root, "P101.HIF");
        scan(&db, root);

        assert_eq!(group_of_path(&db, "P101.ORI").0, 4, "one shot, one group");
        assert_eq!(group_id_of(&db, "P101.ORI"), group_before);
        assert_eq!(group_id_of(&db, "P101.HIF"), group_before);
    }

    /// Decoupling is the user saying "these are separate photos". A scan must
    /// not undo that by pulling a new sibling into the group.
    #[test]
    fn a_latecomer_leaves_a_decoupled_group_alone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "IMG_2.CR3");
        touch(root, "IMG_2.JPG");
        let db = Arc::new(Db::open(root).unwrap());
        scan(&db, root);
        crate::engine::groups::decouple(&db, group_id_of(&db, "IMG_2.CR3")).unwrap();

        touch(root, "IMG_2.HIF");
        scan(&db, root);

        assert_eq!(group_of_path(&db, "IMG_2.CR3").0, 2, "pair untouched");
        assert_eq!(group_of_path(&db, "IMG_2.HIF").0, 1, "stays alone");
    }

    /// `group_size` counts live members, so a group holding a committed delete
    /// can look like a singleton and get absorbed. `files.group_id` is a foreign
    /// key with `foreign_keys = ON`, so dropping that group while the dead row
    /// still points at it fails the scan transaction — and every scan after it.
    #[test]
    fn a_group_holding_a_committed_delete_still_absorbs_a_latecomer() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "IMG_4.CR3");
        touch(root, "IMG_4.HIF");
        let db = Arc::new(Db::open(root).unwrap());
        scan(&db, root);
        std::fs::remove_file(root.join("IMG_4.CR3")).unwrap();
        db.call(|c| {
            c.execute("UPDATE files SET status = 2 WHERE ext = 'cr3'", [])?;
            Ok(())
        })
        .unwrap();

        touch(root, "IMG_4.JPG");
        scan(&db, root);

        assert_eq!(group_of_path(&db, "IMG_4.HIF").1, "IMG_4.JPG");
        assert_eq!(
            group_id_of(&db, "IMG_4.CR3"),
            group_id_of(&db, "IMG_4.HIF"),
            "the deleted row follows its shot instead of dangling"
        );
    }

    /// The other refusal in `merge_group`, and the only branch of it no normal
    /// operation reaches: one basename spanning two groups that both already
    /// hold members. Merging would pick a winner and silently dissolve the
    /// other, so the pass leaves both alone. Driven straight through the DB,
    /// because no command builds this state.
    #[test]
    fn a_bucket_spanning_two_established_groups_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "IMG_5.CR3");
        touch(root, "IMG_5.JPG");
        touch(root, "other/IMG_5.ORF");
        touch(root, "other/IMG_5.ORI");
        let db = Arc::new(Db::open(root).unwrap());
        scan(&db, root);

        // Force the second shot's members into the first shot's dir, so one
        // dir+basename now spans two groups of two.
        db.call(|c| {
            c.execute("UPDATE files SET dir = '' WHERE dir = 'other'", [])?;
            Ok(())
        })
        .unwrap();
        let cr3_group = group_id_of(&db, "IMG_5.CR3");
        let orf_group = group_id_of(&db, "other/IMG_5.ORF");

        scan(&db, root);

        assert_ne!(cr3_group, orf_group, "the two shots started apart");
        assert_eq!(group_id_of(&db, "IMG_5.CR3"), cr3_group, "untouched");
        assert_eq!(group_id_of(&db, "other/IMG_5.ORF"), orf_group, "untouched");
    }

    /// A RAW arriving beside a JPEG-only group takes the cell from it: the
    /// primary is recomputed over the whole membership, not carried over.
    #[test]
    fn a_late_raw_takes_over_as_primary() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "IMG_3.JPG");
        touch(root, "IMG_3.HEIC");
        let db = Arc::new(Db::open(root).unwrap());
        scan(&db, root);
        assert_eq!(group_of_path(&db, "IMG_3.JPG").1, "IMG_3.JPG");

        touch(root, "IMG_3.CR3");
        scan(&db, root);

        assert_eq!(group_of_path(&db, "IMG_3.HEIC").1, "IMG_3.CR3");
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
