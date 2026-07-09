use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::Xxh3;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::store::{split_parent, ProjectStore};

use super::actions::{ActionKind, PendingAction};
use super::xmp::{self, XmpState};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum DeletionMode {
    #[default]
    Recycle,
    Permanent,
    Trash, // project-local _trash folder
}

impl DeletionMode {
    pub fn from_setting(s: &str) -> DeletionMode {
        match s {
            "permanent" => DeletionMode::Permanent,
            "trash" => DeletionMode::Trash,
            _ => DeletionMode::Recycle,
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CommitPlan {
    pub deletes: Vec<PendingAction>,
    pub moves: Vec<PendingAction>,
    pub copies: Vec<PendingAction>,
    pub xmp_count: usize,
    pub deletion_mode: DeletionMode,
    pub conflicts: Vec<String>,
    pub plan_hash: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CommitOutcome {
    pub commit_id: i64,
    pub ok: usize,
    pub errors: usize,
    pub error_samples: Vec<String>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn get_setting(db: &Arc<Db>, key: &str) -> AppResult<Option<String>> {
    let key = key.to_string();
    db.call(move |conn| {
        Ok(conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    })
}

pub fn set_setting(db: &Arc<Db>, key: &str, value: &str) -> AppResult<()> {
    let (key, value) = (key.to_string(), value.to_string());
    db.call(move |conn| {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    })
}

/// (file_id, rel_path, rating, flag, label)
type DirtyRaw = (i64, String, i64, i64, Option<String>);

fn xmp_dirty_raws(db: &Arc<Db>) -> AppResult<Vec<DirtyRaw>> {
    db.call(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, rel_path, rating, flag, label FROM files
             WHERE status = 0 AND kind = 0 AND xmp_dirty = 1",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
}

pub fn preview(db: &Arc<Db>, store: &dyn ProjectStore) -> AppResult<CommitPlan> {
    let pending = super::actions::list(db)?;
    let deletion_mode =
        DeletionMode::from_setting(&get_setting(db, "deletionMode")?.unwrap_or_default());

    let mut deletes = Vec::new();
    let mut moves = Vec::new();
    let mut copies = Vec::new();
    let mut conflicts = Vec::new();

    for p in pending {
        match p.action {
            ActionKind::Delete => deletes.push(p),
            ActionKind::Move | ActionKind::Copy => {
                if let Some(dest) = &p.dest {
                    let file_name = p.rel_path.rsplit('/').next().unwrap_or(&p.rel_path);
                    let dest_rel = format!("{}/{file_name}", dest.trim_matches('/'));
                    if store.exists(&dest_rel).unwrap_or(false) {
                        conflicts.push(format!("{}: target already exists in {dest}", p.rel_path));
                    }
                }
                if p.action == ActionKind::Move {
                    moves.push(p)
                } else {
                    copies.push(p)
                }
            }
        }
    }

    let xmp = xmp_dirty_raws(db)?;

    let mut hasher = Xxh3::new();
    for list in [&deletes, &moves, &copies] {
        for p in list {
            hasher.update(format!("{}:{}:{:?}", p.id, p.file_id, p.dest).as_bytes());
        }
    }
    for (id, ..) in &xmp {
        hasher.update(format!("x{id}").as_bytes());
    }
    hasher.update(format!("{deletion_mode:?}").as_bytes());
    let plan_hash = format!("{:016x}", hasher.digest());

    Ok(CommitPlan {
        deletes,
        moves,
        copies,
        xmp_count: xmp.len(),
        deletion_mode,
        conflicts,
        plan_hash,
    })
}

/// Delete one file (by rel_path) according to the mode, through the store.
/// Returns undo info JSON (with a project-relative trash path when applicable).
fn delete_via_store(
    store: &dyn ProjectStore,
    rel: &str,
    mode: DeletionMode,
) -> AppResult<String> {
    match mode {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        DeletionMode::Recycle => match store.local_path(rel) {
            Some(abs) => {
                trash::delete(&abs).map_err(|e| AppError::Other(format!("recycle: {e}")))?;
                Ok(r#"{"mode":"recycle"}"#.to_string())
            }
            // No real OS path (shouldn't happen on desktop): fall back to trash folder.
            None => store_trash(store, rel),
        },
        // No OS recycle bin API on mobile — fall back to the project-local trash folder.
        #[cfg(any(target_os = "android", target_os = "ios"))]
        DeletionMode::Recycle => store_trash(store, rel),
        DeletionMode::Permanent => {
            store.remove_file(rel)?;
            Ok(r#"{"mode":"permanent"}"#.to_string())
        }
        DeletionMode::Trash => store_trash(store, rel),
    }
}

/// Move a file into the project-local `_trash` folder, preserving its relative
/// directory structure, with `.N` suffixing on collision. Returns undo JSON
/// carrying the (project-relative) trash path.
fn store_trash(store: &dyn ProjectStore, rel: &str) -> AppResult<String> {
    let (parent, name) = split_parent(rel);
    let trash_parent = if parent.is_empty() {
        "_trash".to_string()
    } else {
        format!("_trash/{parent}")
    };
    store.create_dir_all(&trash_parent)?;

    // Pick a non-colliding target name inside the trash folder.
    let mut unique = name.to_string();
    let mut n = 1;
    while store.exists(&format!("{trash_parent}/{unique}"))? {
        unique = format!("{name}.{n}");
        n += 1;
    }
    // move_to keeps the source name, so rename the source first when a suffix
    // is needed, then move it into the trash folder.
    let src_rel = if unique != name {
        store.rename_in_place(rel, &unique)?
    } else {
        rel.to_string()
    };
    let final_rel = store.move_to(&src_rel, &trash_parent)?;
    Ok(serde_json::json!({ "mode": "trash", "trashPath": final_rel }).to_string())
}

pub fn execute(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    plan_hash: &str,
    mut progress: impl FnMut(usize, usize),
) -> AppResult<CommitOutcome> {
    let plan = preview(db, store)?;
    if plan.plan_hash != plan_hash {
        return Err(AppError::Other(
            "pending actions changed since the preview — review again".into(),
        ));
    }

    let xmp_files = xmp_dirty_raws(db)?;
    let total = plan.deletes.len() + plan.moves.len() + plan.copies.len() + xmp_files.len();
    let started = now_secs();

    let summary = serde_json::json!({
        "deletes": plan.deletes.len(),
        "moves": plan.moves.len(),
        "copies": plan.copies.len(),
        "xmp": xmp_files.len(),
        "deletionMode": plan.deletion_mode,
    })
    .to_string();
    let commit_id = db.call(move |conn| {
        conn.execute(
            "INSERT INTO commits (started_at, status, summary) VALUES (?1, 0, ?2)",
            params![started, summary],
        )?;
        Ok(conn.last_insert_rowid())
    })?;

    let mut ok = 0usize;
    let mut errors = 0usize;
    let mut error_samples = Vec::new();
    let mut done = 0usize;

    let record = |db: &Arc<Db>,
                  file_id: Option<i64>,
                  action: i64,
                  before: Option<String>,
                  after: Option<String>,
                  undo: Option<String>,
                  result: Result<(), String>|
     -> AppResult<()> {
        let (res_i, err) = match &result {
            Ok(()) => (0i64, None),
            Err(e) => (2i64, Some(e.clone())),
        };
        db.call(move |conn| {
            conn.execute(
                "INSERT INTO commit_entries
                 (commit_id, file_id, action, before_path, after_path, undo_info, result, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![commit_id, file_id, action, before, after, undo, res_i, err],
            )?;
            Ok(())
        })
    };

    // --- deletes ---
    for p in &plan.deletes {
        let mut result: Result<String, String> = if store.exists(&p.rel_path).unwrap_or(false) {
            delete_via_store(store, &p.rel_path, plan.deletion_mode).map_err(|e| e.to_string())
        } else {
            Err("file missing on disk".into())
        };
        // A RAW's sidecar travels with it.
        if result.is_ok() {
            let sidecar_rel = xmp::sidecar_rel(&p.rel_path);
            if sidecar_rel != p.rel_path && store.exists(&sidecar_rel).unwrap_or(false) {
                if let Err(e) = delete_via_store(store, &sidecar_rel, plan.deletion_mode) {
                    tracing::warn!("sidecar delete failed: {e}");
                }
            }
        }
        match &mut result {
            Ok(undo) => {
                ok += 1;
                let file_id = p.file_id;
                db.call(move |conn| {
                    let tx = conn.transaction()?;
                    tx.execute(
                        "UPDATE files SET status = 2 WHERE id = ?1",
                        params![file_id],
                    )?;
                    tx.execute(
                        "DELETE FROM pending_actions WHERE file_id = ?1",
                        params![file_id],
                    )?;
                    // If it was the group's primary, promote the survivor.
                    tx.execute(
                        "UPDATE groups SET primary_file_id =
                           (SELECT id FROM files WHERE group_id = groups.id AND status = 0 LIMIT 1)
                         WHERE primary_file_id = ?1",
                        params![file_id],
                    )?;
                    tx.commit()?;
                    Ok(())
                })?;
                record(
                    db,
                    Some(p.file_id),
                    0,
                    Some(p.rel_path.clone()),
                    None,
                    Some(undo.clone()),
                    Ok(()),
                )?;
            }
            Err(e) => {
                errors += 1;
                if error_samples.len() < 5 {
                    error_samples.push(format!("{}: {e}", p.rel_path));
                }
                record(
                    db,
                    Some(p.file_id),
                    0,
                    Some(p.rel_path.clone()),
                    None,
                    None,
                    Err(e.clone()),
                )?;
            }
        }
        done += 1;
        progress(done, total);
    }

    // --- moves & copies ---
    for (list, action_i) in [(&plan.moves, 1i64), (&plan.copies, 2i64)] {
        for p in list {
            let dest = p.dest.clone().unwrap_or_default();
            let file_name = p
                .rel_path
                .rsplit('/')
                .next()
                .unwrap_or(&p.rel_path)
                .to_string();
            let dest_dir = dest.trim_matches('/').to_string();
            let dest_rel = if dest_dir.is_empty() {
                file_name.clone()
            } else {
                format!("{dest_dir}/{file_name}")
            };

            let result: Result<(), String> = (|| {
                if !store.exists(&p.rel_path).map_err(|e| e.to_string())? {
                    return Err("file missing on disk".into());
                }
                if store.exists(&dest_rel).map_err(|e| e.to_string())? {
                    return Err(format!("target exists: {dest_rel}"));
                }
                store.create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
                if action_i == 1 {
                    store.move_to(&p.rel_path, &dest_dir).map_err(|e| e.to_string())?;
                } else {
                    store.copy(&p.rel_path, &dest_rel).map_err(|e| e.to_string())?;
                }
                Ok(())
            })();

            match &result {
                Ok(()) => {
                    ok += 1;
                    let file_id = p.file_id;
                    let pending_id = p.id;
                    if action_i == 1 {
                        let new_rel = dest_rel.clone();
                        db.call(move |conn| {
                            let dir = new_rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                            conn.execute(
                                "UPDATE files SET rel_path = ?2, dir = ?3 WHERE id = ?1",
                                params![file_id, new_rel, dir],
                            )?;
                            conn.execute(
                                "DELETE FROM pending_actions WHERE id = ?1",
                                params![pending_id],
                            )?;
                            Ok(())
                        })?;
                    } else {
                        db.call(move |conn| {
                            conn.execute(
                                "DELETE FROM pending_actions WHERE id = ?1",
                                params![pending_id],
                            )?;
                            Ok(())
                        })?;
                    }
                }
                Err(e) => {
                    errors += 1;
                    if error_samples.len() < 5 {
                        error_samples.push(format!("{}: {e}", p.rel_path));
                    }
                }
            }
            record(
                db,
                Some(p.file_id),
                action_i,
                Some(p.rel_path.clone()),
                Some(dest_rel),
                None,
                result,
            )?;
            done += 1;
            progress(done, total);
        }
    }

    // --- XMP sidecars ---
    for (file_id, rel_path, rating, flag, label) in xmp_files {
        let state = XmpState {
            rating,
            flag,
            label,
        };
        let result: Result<String, String> =
            xmp::write_sidecar(store, &rel_path, &state).map_err(|e| e.to_string());
        match &result {
            Ok(sc_rel) => {
                ok += 1;
                db.call(move |conn| {
                    conn.execute(
                        "UPDATE files SET xmp_dirty = 0 WHERE id = ?1",
                        params![file_id],
                    )?;
                    Ok(())
                })?;
                record(
                    db,
                    Some(file_id),
                    3,
                    Some(rel_path.clone()),
                    Some(sc_rel.clone()),
                    None,
                    Ok(()),
                )?;
            }
            Err(e) => {
                errors += 1;
                if error_samples.len() < 5 {
                    error_samples.push(format!("{rel_path}: {e}"));
                }
                record(
                    db,
                    Some(file_id),
                    3,
                    Some(rel_path.clone()),
                    None,
                    None,
                    Err(e.clone()),
                )?;
            }
        }
        done += 1;
        progress(done, total);
    }

    let status = if errors == 0 { 1i64 } else { 2i64 };
    let finished = now_secs();
    db.call(move |conn| {
        conn.execute(
            "UPDATE commits SET finished_at = ?2, status = ?3 WHERE id = ?1",
            params![commit_id, finished, status],
        )?;
        Ok(())
    })?;

    Ok(CommitOutcome {
        commit_id,
        ok,
        errors,
        error_samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::actions::{enqueue, ActionKind, PairScope};
    use crate::engine::culling::Targets;
    use std::fs;

    fn ids(db: &Arc<Db>, ext: &str) -> i64 {
        let ext = ext.to_string();
        db.call(move |c| {
            Ok(
                c.query_row("SELECT id FROM files WHERE ext = ?1", params![ext], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap()
    }

    #[test]
    fn commit_moves_deletes_and_writes_xmp() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("keep.cr3"), b"raw").unwrap();
        fs::write(root.join("bad.jpg"), b"jpg").unwrap();
        fs::write(root.join("sel.jpg"), b"jpg").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        set_setting(&db, "deletionMode", "trash").unwrap();

        // Rate the RAW so it becomes xmp_dirty.
        crate::engine::culling::set_rating(
            &db,
            Targets {
                ids: vec![ids(&db, "cr3")],
                as_groups: false,
            },
            5,
        )
        .unwrap();

        // Delete bad.jpg, move sel.jpg to selects/
        let bad_id: i64 = db
            .call(|c| {
                Ok(
                    c.query_row("SELECT id FROM files WHERE rel_path='bad.jpg'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        let sel_id: i64 = db
            .call(|c| {
                Ok(
                    c.query_row("SELECT id FROM files WHERE rel_path='sel.jpg'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        enqueue(
            &db,
            Targets {
                ids: vec![bad_id],
                as_groups: false,
            },
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        enqueue(
            &db,
            Targets {
                ids: vec![sel_id],
                as_groups: false,
            },
            ActionKind::Move,
            Some("selects".into()),
            PairScope::Both,
        )
        .unwrap();

        let plan = preview(&db, &store).unwrap();
        assert_eq!(plan.deletes.len(), 1);
        assert_eq!(plan.moves.len(), 1);
        assert_eq!(plan.xmp_count, 1);
        assert!(plan.conflicts.is_empty());

        let outcome = execute(&db, &store, &plan.plan_hash, |_, _| {}).unwrap();
        assert_eq!(outcome.errors, 0, "{:?}", outcome.error_samples);
        assert_eq!(outcome.ok, 3);

        assert!(!root.join("bad.jpg").exists());
        assert!(root.join("_trash").join("bad.jpg").exists());
        assert!(root.join("selects").join("sel.jpg").exists());
        assert!(root.join("keep.xmp").exists());

        // DB reflects reality.
        let remaining: i64 = db
            .call(|c| {
                Ok(
                    c.query_row("SELECT COUNT(*) FROM files WHERE status = 0", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(remaining, 2);
        let moved_rel: String = db
            .call(move |c| {
                Ok(c.query_row(
                    "SELECT rel_path FROM files WHERE id = ?1",
                    params![sel_id],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(moved_rel, "selects/sel.jpg");
        let pending_left: i64 = db
            .call(|c| Ok(c.query_row("SELECT COUNT(*) FROM pending_actions", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(pending_left, 0);

        // Stale hash is rejected.
        enqueue(
            &db,
            Targets {
                ids: vec![ids(&db, "cr3")],
                as_groups: false,
            },
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        assert!(execute(&db, &store, &plan.plan_hash, |_, _| {}).is_err());
    }
}
