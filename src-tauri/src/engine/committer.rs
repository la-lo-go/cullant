use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::Xxh3;

use crate::db::Db;
use crate::error::{AppError, AppResult};

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

pub fn preview(db: &Arc<Db>, root: &Path) -> AppResult<CommitPlan> {
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
                    let target = root.join(dest).join(file_name);
                    if target.exists() {
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

/// Delete one path according to the mode. Returns undo info JSON.
fn delete_path(root: &Path, abs: &Path, rel: &str, mode: DeletionMode) -> AppResult<String> {
    match mode {
        DeletionMode::Recycle => {
            trash::delete(abs).map_err(|e| AppError::Other(format!("recycle: {e}")))?;
            Ok(r#"{"mode":"recycle"}"#.to_string())
        }
        DeletionMode::Permanent => {
            std::fs::remove_file(abs)?;
            Ok(r#"{"mode":"permanent"}"#.to_string())
        }
        DeletionMode::Trash => {
            let mut target = root.join("_trash").join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut n = 1;
            while target.exists() {
                target = root.join("_trash").join(format!("{rel}.{n}"));
                n += 1;
            }
            std::fs::rename(abs, &target)?;
            Ok(serde_json::json!({
                "mode": "trash",
                "trashPath": target.to_string_lossy()
            })
            .to_string())
        }
    }
}

pub fn execute(
    db: &Arc<Db>,
    root: &Path,
    plan_hash: &str,
    mut progress: impl FnMut(usize, usize),
) -> AppResult<CommitOutcome> {
    let plan = preview(db, root)?;
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
        let abs = root.join(&p.rel_path);
        let mut result: Result<String, String> = if abs.exists() {
            delete_path(root, &abs, &p.rel_path, plan.deletion_mode).map_err(|e| e.to_string())
        } else {
            Err("file missing on disk".into())
        };
        // A RAW's sidecar travels with it.
        if result.is_ok() {
            let sidecar = xmp::sidecar_path(root, &p.rel_path);
            if sidecar.exists() && sidecar != abs {
                let rel_sidecar = format!(
                    "{}.xmp",
                    p.rel_path
                        .rsplit_once('.')
                        .map(|(a, _)| a)
                        .unwrap_or(&p.rel_path)
                );
                if let Err(e) = delete_path(root, &sidecar, &rel_sidecar, plan.deletion_mode) {
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
            let abs = root.join(&p.rel_path);
            let file_name = p
                .rel_path
                .rsplit('/')
                .next()
                .unwrap_or(&p.rel_path)
                .to_string();
            let dest_rel = format!("{}/{}", dest.trim_matches('/'), file_name);
            let target = root.join(&dest_rel);

            let result: Result<(), String> = (|| {
                if !abs.exists() {
                    return Err("file missing on disk".into());
                }
                if target.exists() {
                    return Err(format!("target exists: {dest_rel}"));
                }
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                if action_i == 1 {
                    std::fs::rename(&abs, &target).map_err(|e| e.to_string())?;
                } else {
                    std::fs::copy(&abs, &target).map_err(|e| e.to_string())?;
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
        let result: Result<PathBuf, String> =
            xmp::write_sidecar(root, &rel_path, &state).map_err(|e| e.to_string());
        match &result {
            Ok(path) => {
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
                    Some(path.to_string_lossy().into_owned()),
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

        let plan = preview(&db, root).unwrap();
        assert_eq!(plan.deletes.len(), 1);
        assert_eq!(plan.moves.len(), 1);
        assert_eq!(plan.xmp_count, 1);
        assert!(plan.conflicts.is_empty());

        let outcome = execute(&db, root, &plan.plan_hash, |_, _| {}).unwrap();
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
        assert!(execute(&db, root, &plan.plan_hash, |_, _| {}).is_err());
    }
}
