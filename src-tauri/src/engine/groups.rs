use std::sync::Arc;

use rusqlite::params;
use serde::Deserialize;

use crate::db::Db;
use crate::error::{AppError, AppResult};

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SyncFrom {
    /// Copy the RAW member's culling state onto the whole group.
    Raw,
    /// Copy the JPEG member's culling state onto the whole group.
    Jpeg,
    /// Keep each member's state as-is (may stay divergent).
    None,
}

pub fn decouple(db: &Arc<Db>, group_id: i64) -> AppResult<()> {
    db.call(move |conn| {
        let n = conn.execute(
            "UPDATE groups SET decoupled = 1 WHERE id = ?1",
            params![group_id],
        )?;
        if n == 0 {
            return Err(AppError::Other(format!("no such group: {group_id}")));
        }
        Ok(())
    })
}

/// Re-link a decoupled pair. Optionally syncs culling state from one member
/// so the pair is consistent again.
pub fn recouple(db: &Arc<Db>, group_id: i64, sync_from: SyncFrom) -> AppResult<()> {
    db.call(move |conn| {
        let tx = conn.transaction()?;
        let n = tx.execute(
            "UPDATE groups SET decoupled = 0 WHERE id = ?1",
            params![group_id],
        )?;
        if n == 0 {
            return Err(AppError::Other(format!("no such group: {group_id}")));
        }

        let source_kind = match sync_from {
            SyncFrom::Raw => Some(0i64),
            SyncFrom::Jpeg => Some(1i64),
            SyncFrom::None => None,
        };
        if let Some(kind) = source_kind {
            tx.execute(
                "UPDATE files SET
                   rating = src.rating, flag = src.flag, label = src.label,
                   xmp_dirty = 1
                 FROM (SELECT rating, flag, label FROM files
                       WHERE group_id = ?1 AND kind = ?2 AND status = 0 LIMIT 1) AS src
                 WHERE files.group_id = ?1 AND files.status = 0",
                params![group_id, kind],
            )?;
        }
        tx.commit()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn recouple_syncs_state_from_raw() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("A.cr3"), b"raw").unwrap();
        fs::write(root.join("A.jpg"), b"jpg").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();

        let group_id: i64 = db
            .call(|c| Ok(c.query_row("SELECT group_id FROM files LIMIT 1", [], |r| r.get(0))?))
            .unwrap();

        decouple(&db, group_id).unwrap();
        db.call(|c| {
            c.execute("UPDATE files SET rating = 5 WHERE kind = 0", [])?;
            c.execute("UPDATE files SET rating = 2 WHERE kind = 1", [])?;
            Ok(())
        })
        .unwrap();

        recouple(&db, group_id, SyncFrom::Raw).unwrap();

        let ratings: Vec<i64> = db
            .call(|c| {
                let mut stmt = c.prepare("SELECT rating FROM files ORDER BY id")?;
                let rows = stmt.query_map([], |r| r.get(0))?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap();
        assert_eq!(ratings, vec![5, 5]);

        let decoupled: i64 = db
            .call(move |c| {
                Ok(c.query_row(
                    "SELECT decoupled FROM groups WHERE id = ?1",
                    params![group_id],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(decoupled, 0);
    }
}
