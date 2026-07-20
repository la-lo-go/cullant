use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::{AppError, AppResult};

use super::culling::Targets;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActionKind {
    Delete = 0,
    Move = 1,
    Copy = 2,
}

impl ActionKind {
    pub fn from_i64(v: i64) -> Option<ActionKind> {
        match v {
            0 => Some(ActionKind::Delete),
            1 => Some(ActionKind::Move),
            2 => Some(ActionKind::Copy),
            _ => None,
        }
    }
}

/// Which members of a pair a delete targets (mirror-mode power move:
/// "keep the JPEG, drop the RAW").
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PairScope {
    #[default]
    Both,
    RawOnly,
    JpegOnly,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PendingAction {
    pub id: i64,
    pub file_id: i64,
    pub rel_path: String,
    pub action: ActionKind,
    pub dest: Option<String>,
    pub pair_token: Option<String>,
    pub origin: i64, // 0=manual 1=rule
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Queue an action. For deletes, `pair_scope` restricts which pair members
/// are hit; move/copy always follow normal fan-out.
pub fn enqueue(
    db: &Arc<Db>,
    targets: Targets,
    action: ActionKind,
    dest: Option<String>,
    pair_scope: PairScope,
) -> AppResult<usize> {
    if matches!(action, ActionKind::Move | ActionKind::Copy) && dest.is_none() {
        return Err(AppError::Other("move/copy needs a destination".into()));
    }
    db.call(move |conn| {
        let tx = conn.transaction()?;
        let mut ids = super::culling::expand_targets(&tx, &targets)?;

        if pair_scope != PairScope::Both {
            let keep_kind = if pair_scope == PairScope::RawOnly { 0i64 } else { 1i64 };
            let mut kind_stmt = tx.prepare_cached("SELECT kind FROM files WHERE id = ?1")?;
            let mut filtered = Vec::new();
            for id in &ids {
                let kind: i64 = kind_stmt.query_row(params![id], |r| r.get(0))?;
                if kind == keep_kind {
                    filtered.push(*id);
                }
            }
            ids = filtered;
        }

        let pair_token = if ids.len() > 1 {
            Some(format!("p{}-{}", ids[0], now_secs()))
        } else {
            None
        };
        let params_json = match &dest {
            Some(d) => serde_json::json!({ "dest": d }).to_string(),
            None => "{}".to_string(),
        };

        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO pending_actions (file_id, action, params, pair_token, origin, created_at)
                 VALUES (?1, ?2, ?3, ?4, 0, ?5)
                 ON CONFLICT(file_id, action) DO UPDATE SET
                   params = excluded.params, pair_token = excluded.pair_token,
                   created_at = excluded.created_at",
            )?;
            for id in &ids {
                stmt.execute(params![id, action as i64, params_json, pair_token, now_secs()])?;
            }
        }
        let count = ids.len();
        tx.commit()?;
        Ok(count)
    })
}

/// Remove queued actions of a given kind for a set of files. Targets get the
/// same group fan-out as enqueue, so unqueueing follows pair semantics
/// (e.g. un-rejecting a photo drops the delete for its RAW+JPEG partner too).
pub fn remove_for_files(db: &Arc<Db>, targets: Targets, action: ActionKind) -> AppResult<usize> {
    db.call(move |conn| {
        let tx = conn.transaction()?;
        let ids = super::culling::expand_targets(&tx, &targets)?;
        let mut count = 0;
        {
            let mut stmt = tx
                .prepare_cached("DELETE FROM pending_actions WHERE file_id = ?1 AND action = ?2")?;
            for id in &ids {
                count += stmt.execute(params![id, action as i64])?;
            }
        }
        tx.commit()?;
        Ok(count)
    })
}

pub fn remove(db: &Arc<Db>, pending_ids: Vec<i64>) -> AppResult<()> {
    db.call(move |conn| {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM pending_actions WHERE id = ?1")?;
            for id in &pending_ids {
                stmt.execute(params![id])?;
            }
        }
        tx.commit()?;
        Ok(())
    })
}

pub fn clear_all(db: &Arc<Db>) -> AppResult<()> {
    db.call(|conn| {
        conn.execute("DELETE FROM pending_actions", [])?;
        Ok(())
    })
}

pub fn list(db: &Arc<Db>) -> AppResult<Vec<PendingAction>> {
    db.call(|conn| {
        let mut stmt = conn.prepare(
            "SELECT p.id, p.file_id, f.rel_path, p.action, p.params, p.pair_token, p.origin
             FROM pending_actions p JOIN files f ON f.id = p.file_id
             ORDER BY p.created_at, p.id",
        )?;
        let rows = stmt.query_map([], |r| {
            let action_i: i64 = r.get(3)?;
            let params_str: String = r.get(4)?;
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                action_i,
                params_str,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, file_id, rel_path, action_i, params_str, pair_token, origin) = row?;
            let Some(action) = ActionKind::from_i64(action_i) else {
                continue;
            };
            let dest = serde_json::from_str::<serde_json::Value>(&params_str)
                .ok()
                .and_then(|v| v.get("dest").and_then(|d| d.as_str()).map(String::from));
            out.push(PendingAction {
                id,
                file_id,
                rel_path,
                action,
                dest,
                pair_token,
                origin,
            });
        }
        Ok(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn raw_only_delete_targets_just_the_raw() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("A.cr3"), b"raw").unwrap();
        fs::write(root.join("A.jpg"), b"jpg").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let jpg_id: i64 = db
            .call(|c| Ok(c.query_row("SELECT id FROM files WHERE ext='jpg'", [], |r| r.get(0))?))
            .unwrap();

        // Target the JPEG (the displayed half), scope raw-only, mirror on:
        // the queued delete must land on the CR3.
        let n = enqueue(
            &db,
            Targets {
                ids: vec![jpg_id],
                as_groups: true,
            },
            ActionKind::Delete,
            None,
            PairScope::RawOnly,
        )
        .unwrap();
        assert_eq!(n, 1);

        let pending = list(&db).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].rel_path.ends_with(".cr3"));
    }

    #[test]
    fn enqueue_pair_delete_shares_token_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("A.cr3"), b"raw").unwrap();
        fs::write(root.join("A.jpg"), b"jpg").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let raw_id: i64 = db
            .call(|c| Ok(c.query_row("SELECT id FROM files WHERE ext='cr3'", [], |r| r.get(0))?))
            .unwrap();

        let n = enqueue(
            &db,
            Targets {
                ids: vec![raw_id],
                as_groups: true,
            },
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        assert_eq!(n, 2);
        let pending = list(&db).unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending[0].pair_token.is_some());
        assert_eq!(pending[0].pair_token, pending[1].pair_token);

        // Re-enqueue replaces rather than duplicating.
        enqueue(
            &db,
            Targets {
                ids: vec![raw_id],
                as_groups: true,
            },
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        assert_eq!(list(&db).unwrap().len(), 2);
    }

    #[test]
    fn remove_for_files_follows_group_fanout_and_keeps_others() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("A.cr3"), b"raw").unwrap();
        fs::write(root.join("A.jpg"), b"jpg").unwrap();
        fs::write(root.join("B.jpg"), b"jpg").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let raw_id: i64 = db
            .call(|c| Ok(c.query_row("SELECT id FROM files WHERE ext='cr3'", [], |r| r.get(0))?))
            .unwrap();

        // Queue deletes for the A pair (mirror fan-out) and for solo B.
        enqueue(
            &db,
            Targets {
                ids: vec![raw_id],
                as_groups: true,
            },
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        let b_id: i64 = db
            .call(|c| {
                Ok(
                    c.query_row("SELECT id FROM files WHERE rel_path='B.jpg'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        enqueue(
            &db,
            Targets {
                ids: vec![b_id],
                as_groups: false,
            },
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        assert_eq!(list(&db).unwrap().len(), 3);

        // Unqueue by targeting one pair member with groups on: both halves go.
        let n = remove_for_files(
            &db,
            Targets {
                ids: vec![raw_id],
                as_groups: true,
            },
            ActionKind::Delete,
        )
        .unwrap();
        assert_eq!(n, 2);

        let pending = list(&db).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].rel_path, "B.jpg");

        // With groups off only the targeted file is unqueued.
        enqueue(
            &db,
            Targets {
                ids: vec![raw_id],
                as_groups: true,
            },
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        let n = remove_for_files(
            &db,
            Targets {
                ids: vec![raw_id],
                as_groups: false,
            },
            ActionKind::Delete,
        )
        .unwrap();
        assert_eq!(n, 1);
        assert_eq!(list(&db).unwrap().len(), 2);
    }
}
