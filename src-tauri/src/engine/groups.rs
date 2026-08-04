use std::sync::Arc;

use rusqlite::{params, Connection};
use serde::Deserialize;

use crate::db::Db;
use crate::engine::culling::{now_secs, read_states, CullState};
use crate::error::{AppError, AppResult};

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SyncFrom {
    /// Copy the RAW member's culling state onto the whole group.
    Raw,
    /// Copy the JPEG member's culling state onto the whole group.
    Jpeg,
    /// Copy the most recently edited member's state onto the whole group.
    Latest,
    /// Keep each member's state as-is (may stay divergent).
    None,
}

/// Copy one member's rating/flag/label onto every present member of a group.
///
/// Takes a connection rather than the `Db` so it can run inside a caller's
/// transaction: recoupling syncs as part of re-linking the pair, and the two
/// must not be separable by a crash in between.
fn sync_members(conn: &Connection, group_id: i64, from: SyncFrom) -> rusqlite::Result<()> {
    let source = match from {
        SyncFrom::Raw => {
            "SELECT rating, flag, label FROM files
               WHERE group_id = ?1 AND kind = 0 AND status = 0 LIMIT 1"
        }
        SyncFrom::Jpeg => {
            "SELECT rating, flag, label FROM files
               WHERE group_id = ?1 AND kind = 1 AND status = 0 LIMIT 1"
        }
        // A member never classified has a NULL timestamp, which must lose to one
        // that has been; the id breaks a tie within the same second.
        SyncFrom::Latest => {
            "SELECT rating, flag, label FROM files
               WHERE group_id = ?1 AND status = 0
               ORDER BY COALESCE(state_updated_at, 0) DESC, id DESC LIMIT 1"
        }
        SyncFrom::None => return Ok(()),
    };
    conn.execute(
        &format!(
            "UPDATE files SET
               rating = src.rating, flag = src.flag, label = src.label,
               state_updated_at = ?2,
               xmp_dirty = CASE WHEN kind IN (0, 1) THEN 1 ELSE xmp_dirty END
             FROM ({source}) AS src
             WHERE files.group_id = ?1 AND files.status = 0"
        ),
        params![group_id, now_secs()],
    )?;
    Ok(())
}

/// Make a pair agree again, without touching whether it is coupled.
///
/// Fan-out already keeps a mirrored pair in step, but a pair can still diverge —
/// edited in separate mode, or while decoupled — and until now the only way back
/// was to decouple and recouple it. Returns the resulting rows, like every other
/// state write, so the caller reconciles them the same way.
pub fn sync_state(db: &Arc<Db>, group_id: i64, from: SyncFrom) -> AppResult<Vec<CullState>> {
    db.call(move |conn| {
        let tx = conn.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM groups WHERE id = ?1)",
            params![group_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(AppError::Other(format!("no such group: {group_id}")));
        }
        sync_members(&tx, group_id, from)?;
        let ids: Vec<i64> = {
            let mut stmt =
                tx.prepare("SELECT id FROM files WHERE group_id = ?1 AND status = 0 ORDER BY id")?;
            let rows = stmt.query_map(params![group_id], |r| r.get(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let out = read_states(&tx, &ids)?;
        tx.commit()?;
        Ok(out)
    })
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

        sync_members(&tx, group_id, sync_from)?;
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

    /// A paired project with the group id, ready to have state forced onto it.
    fn paired_project() -> (tempfile::TempDir, Arc<Db>, i64) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("A.cr3"), b"raw").unwrap();
        fs::write(root.join("A.jpg"), b"jpg").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let group_id: i64 = db
            .call(|c| Ok(c.query_row("SELECT group_id FROM files LIMIT 1", [], |r| r.get(0))?))
            .unwrap();
        (dir, db, group_id)
    }

    #[test]
    fn sync_state_settles_a_coupled_pair_without_decoupling_it() {
        let (_dir, db, group_id) = paired_project();
        db.call(|c| {
            c.execute("UPDATE files SET rating = 4, flag = 1 WHERE kind = 0", [])?;
            c.execute("UPDATE files SET rating = 0, flag = -1 WHERE kind = 1", [])?;
            Ok(())
        })
        .unwrap();

        let states = sync_state(&db, group_id, SyncFrom::Jpeg).unwrap();

        assert_eq!(states.len(), 2);
        assert!(states.iter().all(|s| s.rating == 0 && s.flag == -1));
        // The pair was never decoupled, so recoupling was not the price of this.
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

    #[test]
    fn sync_state_from_latest_follows_the_newer_edit() {
        let (_dir, db, group_id) = paired_project();
        // The JPEG was classified later, so its state is the one that survives —
        // even though the RAW is the group's primary.
        db.call(|c| {
            c.execute(
                "UPDATE files SET rating = 5, state_updated_at = 1000 WHERE kind = 0",
                [],
            )?;
            c.execute(
                "UPDATE files SET rating = 2, state_updated_at = 2000 WHERE kind = 1",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let states = sync_state(&db, group_id, SyncFrom::Latest).unwrap();

        assert!(states.iter().all(|s| s.rating == 2));
    }

    #[test]
    fn sync_state_from_latest_ignores_a_member_never_classified() {
        let (_dir, db, group_id) = paired_project();
        // A NULL timestamp means "never touched" and must lose to any real edit,
        // however old — otherwise a fresh JPEG would wipe the RAW's rating.
        db.call(|c| {
            c.execute(
                "UPDATE files SET rating = 3, state_updated_at = 50 WHERE kind = 0",
                [],
            )?;
            c.execute(
                "UPDATE files SET rating = 0, state_updated_at = NULL WHERE kind = 1",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        let states = sync_state(&db, group_id, SyncFrom::Latest).unwrap();

        assert!(states.iter().all(|s| s.rating == 3));
    }

    #[test]
    fn sync_state_marks_both_members_for_xmp_export() {
        let (_dir, db, group_id) = paired_project();
        db.call(|c| {
            c.execute("UPDATE files SET xmp_dirty = 0", [])?;
            c.execute("UPDATE files SET rating = 4 WHERE kind = 0", [])?;
            Ok(())
        })
        .unwrap();

        sync_state(&db, group_id, SyncFrom::Raw).unwrap();

        let dirty: i64 = db
            .call(|c| {
                Ok(
                    c.query_row("SELECT COUNT(*) FROM files WHERE xmp_dirty = 1", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(dirty, 2);
    }
}
