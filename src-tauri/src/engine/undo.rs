//! Reading the commit history back, and reversing it.
//!
//! Every commit already writes a `commit_entries` row per file describing what
//! happened and where the bytes went; this module is the reader for that trail
//! and the code that walks it backwards.
//!
//! What can and cannot be reversed is a property of how the commit disposed of
//! the file, not a policy choice here:
//!
//! - a delete in `_trash` mode moved the file, so it can be moved back;
//! - a delete in permanent mode unlinked it, and nothing on disk remains;
//! - a move is a move, so it reverses;
//! - a copy is undone by removing the copy;
//! - an XMP write merged into the sidecar in place and no prior bytes were
//!   kept, so it cannot be reversed.
//!
//! The UI is expected to say which of these applies rather than offering a
//! button that quietly does nothing.

use std::sync::Arc;

use rusqlite::params;
use serde::Serialize;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::store::{split_parent, ProjectStore};

use super::committer::{file_fingerprint, SidecarChange, UndoInfo};
use super::xmp;

/// Action codes as written by the committer into `commit_entries.action`.
const ACTION_DELETE: i64 = 0;
const ACTION_MOVE: i64 = 1;
const ACTION_COPY: i64 = 2;
const ACTION_XMP: i64 = 3;

/// A commit as the history list shows it.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommitSummary {
    pub id: i64,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    /// 0 = running, 1 = done, 2 = finished with errors.
    pub status: i64,
    pub deletes: i64,
    pub moves: i64,
    pub copies: i64,
    pub xmp: i64,
    pub errors: i64,
    /// How many entries could still be reversed right now.
    pub undoable: i64,
    pub undone_at: Option<i64>,
}

/// One file's fate within a commit.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommitEntry {
    pub id: i64,
    pub file_id: Option<i64>,
    pub action: i64,
    pub before_path: Option<String>,
    pub after_path: Option<String>,
    /// 0 = ok, 2 = error.
    pub result: i64,
    pub error: Option<String>,
    pub undone_at: Option<i64>,
    pub undoable: bool,
    /// Why it cannot be undone, when it cannot. Shown verbatim in the UI, so it
    /// explains rather than just disabling a button.
    pub blocked_reason: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct UndoOutcome {
    pub restored: usize,
    pub skipped: usize,
    pub errors: usize,
    pub error_samples: Vec<String>,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Whether an entry's effect can still be walked back, and why not when it
/// cannot. A single place so the list, the detail view and the executor can
/// never disagree about what is offered.
fn reversibility(
    action: i64,
    result: i64,
    undone_at: Option<i64>,
    undo: &UndoInfo,
) -> (bool, Option<String>) {
    if undone_at.is_some() {
        return (false, Some("Already undone.".into()));
    }
    if result != 0 {
        return (
            false,
            Some("This entry failed. Check its error and recorded paths.".into()),
        );
    }
    match action {
        ACTION_DELETE => match undo.mode.as_deref() {
            Some("trash") if undo.trash_path.is_some() => (true, None),
            Some("permanent") => (
                false,
                Some("Deleted permanently. The file is gone from disk.".into()),
            ),
            _ => (false, Some("No record of where this file went.".into())),
        },
        ACTION_MOVE => (true, None),
        ACTION_COPY if undo.copy_fingerprint.is_some() => (true, None),
        ACTION_COPY => (
            false,
            Some("The copy identity was not recorded. Remove it manually.".into()),
        ),
        ACTION_XMP => (
            false,
            Some("Sidecar writes merge in place. The previous contents were not kept.".into()),
        ),
        _ => (false, Some("Unknown action.".into())),
    }
}

pub fn list_commits(db: &Arc<Db>) -> AppResult<Vec<CommitSummary>> {
    db.call_read(|conn| {
        let mut stmt = conn.prepare(
            "WITH parsed AS (
               SELECT commit_id, action, result, undone_at,
                      CASE WHEN json_valid(undo_info) THEN undo_info ELSE '{}' END AS info
               FROM commit_entries
             ), entries AS (
               SELECT commit_id, action, result, undone_at,
                      CASE WHEN json_type(info) = 'object'
                        AND COALESCE(json_type(info, '$.mode'), 'missing') IN ('missing', 'null', 'text')
                        AND COALESCE(json_type(info, '$.trashPath'), 'missing') IN ('missing', 'null', 'text')
                        AND COALESCE(json_type(info, '$.sidecarTrashPath'), 'missing') IN ('missing', 'null', 'text')
                        AND COALESCE(json_type(info, '$.copyFingerprint'), 'missing') IN ('missing', 'null', 'text')
                        AND COALESCE(json_type(info, '$.sidecarMoved'), 'missing') IN ('missing', 'true', 'false')
                        AND COALESCE(json_type(info, '$.wasPrimary'), 'missing') IN ('missing', 'true', 'false')
                        AND COALESCE(json_type(info, '$.sidecars'), 'missing') IN ('missing', 'array')
                        AND NOT EXISTS (
                          SELECT 1 FROM json_each(info, '$.sidecars') sc
                          WHERE CASE WHEN sc.type != 'object' THEN 1 ELSE
                            json_type(sc.value, '$.beforePath') IS NOT 'text'
                            OR COALESCE(json_type(sc.value, '$.afterPath'), 'missing') NOT IN ('missing', 'null', 'text')
                          END)
                      THEN info ELSE '{}' END AS info
               FROM parsed
             )
             SELECT c.id, c.started_at, c.finished_at, c.status, c.undone_at,
                    COALESCE(SUM(e.action = 0), 0), COALESCE(SUM(e.action = 1), 0),
                    COALESCE(SUM(e.action = 2), 0), COALESCE(SUM(e.action = 3), 0),
                    COALESCE(SUM(e.result != 0), 0),
                    SUM(CASE WHEN e.result = 0 AND e.undone_at IS NULL THEN
                      CASE e.action
                        WHEN 1 THEN 1
                        WHEN 2 THEN CASE WHEN json_type(e.info, '$.copyFingerprint') = 'text' THEN 1 ELSE 0 END
                        WHEN 0 THEN CASE WHEN json_extract(e.info, '$.mode') = 'trash'
                          AND json_type(e.info, '$.trashPath') = 'text' THEN 1 ELSE 0 END
                        ELSE 0 END
                      ELSE 0 END)
             FROM commits c LEFT JOIN entries e ON e.commit_id = c.id
             GROUP BY c.id ORDER BY c.id DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(CommitSummary {
                id: r.get(0)?,
                started_at: r.get(1)?,
                finished_at: r.get(2)?,
                status: r.get(3)?,
                undone_at: r.get(4)?,
                deletes: r.get(5)?,
                moves: r.get(6)?,
                copies: r.get(7)?,
                xmp: r.get(8)?,
                errors: r.get(9)?,
                undoable: r.get(10)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
}

pub fn commit_detail(db: &Arc<Db>, commit_id: i64) -> AppResult<Vec<CommitEntry>> {
    db.call_read(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, file_id, action, before_path, after_path, result, error,
                    undone_at, undo_info
             FROM commit_entries WHERE commit_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![commit_id], |r| {
            let action: i64 = r.get(2)?;
            let result: i64 = r.get(5)?;
            let undone_at: Option<i64> = r.get(7)?;
            let undo =
                UndoInfo::from_json(r.get::<_, Option<String>>(8)?.as_deref().unwrap_or("{}"));
            let (undoable, blocked_reason) = reversibility(action, result, undone_at, &undo);
            Ok(CommitEntry {
                id: r.get(0)?,
                file_id: r.get(1)?,
                action,
                before_path: r.get(3)?,
                after_path: r.get(4)?,
                result,
                error: r.get(6)?,
                undone_at,
                undoable,
                blocked_reason,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
}

/// Everything the executor needs about one entry, read in one go.
struct Reversal {
    id: i64,
    commit_id: i64,
    file_id: Option<i64>,
    action: i64,
    before_path: Option<String>,
    after_path: Option<String>,
    undo: UndoInfo,
}

fn load_reversals(db: &Arc<Db>, sql: &str, key: i64) -> AppResult<Vec<Reversal>> {
    let sql = sql.to_string();
    db.call_read(move |conn| {
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![key], |r| {
            let action: i64 = r.get(3)?;
            let result: i64 = r.get(7)?;
            let undone_at: Option<i64> = r.get(8)?;
            let undo =
                UndoInfo::from_json(r.get::<_, Option<String>>(6)?.as_deref().unwrap_or("{}"));
            Ok((
                Reversal {
                    id: r.get(0)?,
                    commit_id: r.get(1)?,
                    file_id: r.get(2)?,
                    action,
                    before_path: r.get(4)?,
                    after_path: r.get(5)?,
                    undo,
                },
                result,
                undone_at,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (rev, result, undone_at) = row?;
            if reversibility(rev.action, result, undone_at, &rev.undo).0 {
                out.push(rev);
            }
        }
        Ok(out)
    })
}

const REVERSAL_COLUMNS: &str = "SELECT id, commit_id, file_id, action, before_path, after_path,
                                       undo_info, result, undone_at
                                FROM commit_entries";

/// Undo every still-reversible entry of one commit.
pub fn undo_commit(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    commit_id: i64,
) -> AppResult<UndoOutcome> {
    let sql = format!("{REVERSAL_COLUMNS} WHERE commit_id = ?1");
    let mut items = load_reversals(db, &sql, commit_id)?;
    let total: i64 = db.call_read(move |conn| {
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM commit_entries WHERE commit_id = ?1",
            params![commit_id],
            |r| r.get(0),
        )?)
    })?;
    let skipped = (total as usize).saturating_sub(items.len());
    // Walk the commit backwards: XMP, copies, moves, deletes. A move that
    // followed a delete must be reversed before the delete puts a file back at
    // the path the move vacated.
    items.sort_by_key(|r| (-r.action, -r.id));
    let mut outcome = run(db, store, items)?;
    outcome.skipped = skipped;
    Ok(outcome)
}

/// Undo one entry on its own.
pub fn undo_entry(db: &Arc<Db>, store: &dyn ProjectStore, entry_id: i64) -> AppResult<UndoOutcome> {
    let sql = format!("{REVERSAL_COLUMNS} WHERE id = ?1");
    let items = load_reversals(db, &sql, entry_id)?;
    if items.is_empty() {
        return Err(AppError::Other(
            "that entry cannot be undone (already undone, failed, or not reversible)".into(),
        ));
    }
    run(db, store, items)
}

fn run(db: &Arc<Db>, store: &dyn ProjectStore, items: Vec<Reversal>) -> AppResult<UndoOutcome> {
    let mut outcome = UndoOutcome::default();
    let mut touched_commits: Vec<i64> = Vec::new();

    for item in items {
        let label = item
            .before_path
            .clone()
            .or_else(|| item.after_path.clone())
            .unwrap_or_else(|| format!("entry {}", item.id));
        match reverse_one(db, store, &item) {
            Ok(()) => {
                outcome.restored += 1;
                let entry_id = item.id;
                let at = now_secs();
                db.call(move |conn| {
                    conn.execute(
                        "UPDATE commit_entries SET undone_at = ?2 WHERE id = ?1",
                        params![entry_id, at],
                    )?;
                    Ok(())
                })?;
                if !touched_commits.contains(&item.commit_id) {
                    touched_commits.push(item.commit_id);
                }
            }
            Err(e) => {
                outcome.errors += 1;
                if outcome.error_samples.len() < 5 {
                    outcome.error_samples.push(format!("{label}: {e}"));
                }
            }
        }
    }

    // A commit counts as undone once nothing reversible is left in it.
    for commit_id in touched_commits {
        let sql = format!("{REVERSAL_COLUMNS} WHERE commit_id = ?1");
        if load_reversals(db, &sql, commit_id)?.is_empty() {
            let at = now_secs();
            db.call(move |conn| {
                conn.execute(
                    "UPDATE commits SET undone_at = ?2 WHERE id = ?1",
                    params![commit_id, at],
                )?;
                Ok(())
            })?;
        }
    }

    Ok(outcome)
}

fn reverse_one(db: &Arc<Db>, store: &dyn ProjectStore, item: &Reversal) -> AppResult<()> {
    match item.action {
        ACTION_DELETE => reverse_delete(db, store, item),
        ACTION_MOVE => reverse_move(db, store, item),
        ACTION_COPY => reverse_copy(store, item),
        _ => Err(AppError::Other("not a reversible action".into())),
    }
}

/// Move a file from `from` back to the exact path `to`, creating the parent
/// directory. Returns the path it actually landed at, which differs from `to`
/// only when something else has since taken that name.
fn restore_to(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    from: &str,
    to: &str,
    file_id: Option<i64>,
) -> AppResult<String> {
    let (parent, name) = split_parent(to);
    store.create_dir_all(parent)?;

    // Never overwrite whatever occupies the old path now — the point of an undo
    // is to lose nothing. Same `.N` suffixing the trash uses on collision.
    let mut unique = name.to_string();
    let mut n = 1;
    while store.exists(&join_rel(parent, &unique))?
        || reserved_path(db, &join_rel(parent, &unique), file_id)?
    {
        unique = crate::store::collision_name(name, n);
        n += 1;
    }
    let destination = join_rel(parent, &unique);
    store.move_file(from, &destination)?;
    Ok(destination)
}

fn reserved_path(db: &Arc<Db>, path: &str, file_id: Option<i64>) -> AppResult<bool> {
    let path = path.to_string();
    db.call_read(move |conn| {
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM files WHERE rel_path = ?1 AND id != COALESCE(?2, -1))",
            params![path, file_id],
            |r| r.get(0),
        )?)
    })
}

fn join_rel(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

fn reverse_delete(db: &Arc<Db>, store: &dyn ProjectStore, item: &Reversal) -> AppResult<()> {
    let trash_path = item
        .undo
        .trash_path
        .as_deref()
        .ok_or_else(|| AppError::Other("no trash path recorded".into()))?;
    let original = item
        .before_path
        .as_deref()
        .ok_or_else(|| AppError::Other("no original path recorded".into()))?;

    if !store.exists(trash_path)? {
        return Err(AppError::Other(format!(
            "not in the trash any more: {trash_path}"
        )));
    }
    restore_entry(db, store, item, trash_path, original)
}

fn reverse_move(db: &Arc<Db>, store: &dyn ProjectStore, item: &Reversal) -> AppResult<()> {
    let current = item
        .after_path
        .as_deref()
        .ok_or_else(|| AppError::Other("no destination recorded".into()))?;
    let original = item
        .before_path
        .as_deref()
        .ok_or_else(|| AppError::Other("no original path recorded".into()))?;

    if !store.exists(current)? {
        return Err(AppError::Other(format!("no longer at {current}")));
    }
    restore_entry(db, store, item, current, original)
}

fn restore_entry(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    item: &Reversal,
    from: &str,
    original: &str,
) -> AppResult<()> {
    let restored = restore_to(db, store, from, original, item.file_id)?;
    let mut moved_sidecars = Vec::new();
    let result = (|| {
        restore_sidecars(store, item, from, original, &restored, &mut moved_sidecars)?;
        let Some(file_id) = item.file_id else {
            return Ok(());
        };
        let restored = restored.clone();
        let was_primary = item.action == ACTION_DELETE && item.undo.was_primary;
        db.call(move |conn| {
            let tx = conn.transaction()?;
            let (dir, _) = split_parent(&restored);
            tx.execute("UPDATE files SET status = 0, rel_path = ?2, dir = ?3 WHERE id = ?1", params![file_id, restored, dir])?;
            if was_primary {
                tx.execute("UPDATE groups SET primary_file_id = ?1 WHERE id = (SELECT group_id FROM files WHERE id = ?1)", params![file_id])?;
            }
            tx.commit()?;
            Ok(())
        })
    })();
    if let Err(error) = result {
        let mut message = error.to_string();
        for (source, target) in moved_sidecars.iter().rev() {
            if let Err(e) = store.move_file(target, source) {
                message.push_str(&format!("; sidecar rollback failed: {e}"));
            }
        }
        if let Err(e) = store.move_file(&restored, from) {
            message.push_str(&format!("; media rollback failed: {e}"));
        }
        return Err(AppError::Other(message));
    }
    Ok(())
}

fn restore_sidecars(
    store: &dyn ProjectStore,
    item: &Reversal,
    from: &str,
    original: &str,
    restored: &str,
    moved: &mut Vec<(String, String)>,
) -> AppResult<()> {
    let mut sidecars = item.undo.sidecars.clone();
    if sidecars.is_empty() {
        let legacy = if item.action == ACTION_DELETE {
            item.undo.sidecar_trash_path.clone()
        } else if item.undo.sidecar_moved {
            Some(xmp::sidecar_rel(from))
        } else {
            None
        };
        if let Some(after) = legacy {
            let own_path = xmp::sidecar_rel_per_file(original);
            let (_, own_name) = split_parent(&own_path);
            let (_, recorded_name) = split_parent(&after);
            let before = if recorded_name == own_name
                || recorded_name.starts_with(&format!("{own_name}."))
            {
                xmp::sidecar_rel_per_file(original)
            } else {
                xmp::sidecar_rel(original)
            };
            sidecars.push(SidecarChange {
                before_path: before,
                after_path: Some(after),
            });
        }
    }
    let mut endpoints = Vec::new();
    for sc in sidecars {
        let Some(after) = sc.after_path else {
            continue;
        };
        let target = if sc.before_path == xmp::sidecar_rel_per_file(original) {
            xmp::sidecar_rel_per_file(restored)
        } else {
            xmp::sidecar_rel(restored)
        };
        if !store.exists(&after)? {
            return Err(AppError::Other(format!("sidecar is missing: {after}")));
        }
        if store.exists(&target)? {
            return Err(AppError::Other(format!("sidecar target exists: {target}")));
        }
        endpoints.push((after, target));
    }
    for (after, target) in endpoints {
        store.move_file(&after, &target)?;
        moved.push((after, target));
    }
    Ok(())
}

/// Undoing a copy removes the copy. The source was never touched, and the
/// committer creates no `files` row for the destination (a rescan would), so
/// there is nothing to reverse in the database.
fn reverse_copy(store: &dyn ProjectStore, item: &Reversal) -> AppResult<()> {
    let copy = item
        .after_path
        .as_deref()
        .ok_or_else(|| AppError::Other("no destination recorded".into()))?;
    if !store.exists(copy)? {
        return Err(AppError::Other(format!("no longer at {copy}")));
    }
    let expected = item
        .undo
        .copy_fingerprint
        .as_deref()
        .ok_or_else(|| AppError::Other("copy identity was not recorded".into()))?;
    if file_fingerprint(store, copy)? != expected {
        return Err(AppError::Other(format!(
            "copy was changed or replaced: {copy}"
        )));
    }
    store.remove_file(copy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::actions::{enqueue, ActionKind, PairScope};
    use crate::engine::committer::{execute, preview, set_setting};
    use crate::engine::culling::{set_rating, Targets};
    use crate::store::LocalFsStore;
    use std::fs;
    use std::path::Path;

    struct Fixture {
        db: Arc<Db>,
        store: LocalFsStore,
        _dir: tempfile::TempDir,
    }

    /// A scanned project containing `names`, deleting into `_trash` by default.
    fn project(names: &[&str]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root: &Path = dir.path();
        for name in names {
            if let Some((sub, _)) = name.rsplit_once('/') {
                fs::create_dir_all(root.join(sub)).unwrap();
            }
            fs::write(root.join(name), b"data").unwrap();
        }
        let db = Arc::new(Db::open(root).unwrap());
        let store = LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        set_setting(&db, "deletionMode", "trash").unwrap();
        Fixture {
            db,
            store,
            _dir: dir,
        }
    }

    impl Fixture {
        fn root(&self) -> &Path {
            self._dir.path()
        }

        fn id_of(&self, rel: &str) -> i64 {
            let rel = rel.to_string();
            self.db
                .call(move |c| {
                    Ok(c.query_row(
                        "SELECT id FROM files WHERE rel_path = ?1",
                        params![rel],
                        |r| r.get(0),
                    )?)
                })
                .unwrap()
        }

        fn targets(&self, rel: &str) -> Targets {
            Targets {
                ids: vec![self.id_of(rel)],
                as_groups: false,
            }
        }

        fn commit(&self) {
            let plan = preview(&self.db, &self.store).unwrap();
            let outcome = execute(&self.db, &self.store, &plan.plan_hash, |_, _, _| {}).unwrap();
            assert_eq!(outcome.errors, 0, "{:?}", outcome.error_samples);
        }

        fn status_of(&self, file_id: i64) -> i64 {
            self.db
                .call(move |c| {
                    Ok(c.query_row(
                        "SELECT status FROM files WHERE id = ?1",
                        params![file_id],
                        |r| r.get(0),
                    )?)
                })
                .unwrap()
        }

        fn rel_path_of(&self, file_id: i64) -> String {
            self.db
                .call(move |c| {
                    Ok(c.query_row(
                        "SELECT rel_path FROM files WHERE id = ?1",
                        params![file_id],
                        |r| r.get(0),
                    )?)
                })
                .unwrap()
        }

        fn last_commit(&self) -> i64 {
            self.db
                .call(|c| Ok(c.query_row("SELECT MAX(id) FROM commits", [], |r| r.get(0))?))
                .unwrap()
        }
    }

    #[test]
    fn trashed_delete_comes_back_with_its_row_and_primary() {
        let f = project(&["shoot/keep.jpg"]);
        let id = f.id_of("shoot/keep.jpg");
        enqueue(
            &f.db,
            f.targets("shoot/keep.jpg"),
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        f.commit();
        assert!(!f.root().join("shoot/keep.jpg").exists());
        assert_eq!(f.status_of(id), 2);

        let outcome = undo_commit(&f.db, &f.store, f.last_commit()).unwrap();
        assert_eq!((outcome.restored, outcome.errors), (1, 0));

        assert!(f.root().join("shoot/keep.jpg").exists(), "file is back");
        assert_eq!(f.status_of(id), 0, "row is present again");
        assert_eq!(f.rel_path_of(id), "shoot/keep.jpg");
        // It was its singleton group's primary before the delete promoted away.
        let primary: Option<i64> =
            f.db.call(move |c| {
                Ok(c.query_row(
                    "SELECT primary_file_id FROM groups
                     WHERE id = (SELECT group_id FROM files WHERE id = ?1)",
                    params![id],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(primary, Some(id), "primary handed back");
    }

    #[test]
    fn permanent_delete_is_refused_not_silently_skipped() {
        let f = project(&["gone.jpg"]);
        set_setting(&f.db, "deletionMode", "permanent").unwrap();
        enqueue(
            &f.db,
            f.targets("gone.jpg"),
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        f.commit();

        let entries = commit_detail(&f.db, f.last_commit()).unwrap();
        let entry = entries.iter().find(|e| e.action == ACTION_DELETE).unwrap();
        assert!(!entry.undoable);
        let reason = entry.blocked_reason.as_deref().unwrap_or("");
        assert!(
            reason.contains("permanently"),
            "the reason must say why, got {reason:?}"
        );
        // The whole-commit undo restores nothing rather than reporting success.
        let outcome = undo_commit(&f.db, &f.store, f.last_commit()).unwrap();
        assert_eq!(outcome.restored, 0);
        // And the single-entry path refuses outright.
        assert!(undo_entry(&f.db, &f.store, entry.id).is_err());
    }

    #[test]
    fn move_goes_back_where_it_came_from() {
        let f = project(&["a.jpg"]);
        let id = f.id_of("a.jpg");
        enqueue(
            &f.db,
            f.targets("a.jpg"),
            ActionKind::Move,
            Some("selects".into()),
            PairScope::Both,
        )
        .unwrap();
        f.commit();
        assert!(f.root().join("selects/a.jpg").exists());
        assert_eq!(f.rel_path_of(id), "selects/a.jpg");

        undo_commit(&f.db, &f.store, f.last_commit()).unwrap();
        assert!(f.root().join("a.jpg").exists(), "back at the original path");
        assert!(!f.root().join("selects/a.jpg").exists());
        assert_eq!(f.rel_path_of(id), "a.jpg", "row follows the file back");
    }

    #[test]
    fn undoing_a_copy_removes_the_copy_and_keeps_the_source() {
        let f = project(&["a.jpg"]);
        enqueue(
            &f.db,
            f.targets("a.jpg"),
            ActionKind::Copy,
            Some("dupes".into()),
            PairScope::Both,
        )
        .unwrap();
        f.commit();
        assert!(f.root().join("dupes/a.jpg").exists());

        undo_commit(&f.db, &f.store, f.last_commit()).unwrap();
        assert!(!f.root().join("dupes/a.jpg").exists(), "copy removed");
        assert!(f.root().join("a.jpg").exists(), "source untouched");
    }

    #[test]
    fn restoring_into_an_occupied_path_never_overwrites() {
        let f = project(&["a.jpg"]);
        enqueue(
            &f.db,
            f.targets("a.jpg"),
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        f.commit();
        // Something else takes the name while the original sits in the trash.
        fs::write(f.root().join("a.jpg"), b"a different file").unwrap();

        let outcome = undo_commit(&f.db, &f.store, f.last_commit()).unwrap();
        assert_eq!((outcome.restored, outcome.errors), (1, 0));
        assert_eq!(
            fs::read(f.root().join("a.jpg")).unwrap(),
            b"a different file",
            "the occupant must survive untouched"
        );
        assert!(
            f.root().join("a.1.jpg").exists(),
            "the restored file lands beside it"
        );
    }

    #[test]
    fn data_regression_undo_collision_preserves_intermediate_source() {
        let f = project(&["a.jpg"]);
        enqueue(
            &f.db,
            f.targets("a.jpg"),
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        f.commit();
        fs::write(f.root().join("a.jpg"), b"replacement").unwrap();
        fs::write(f.root().join("_trash/a.1.jpg"), b"foreign trash").unwrap();
        let outcome = undo_commit(&f.db, &f.store, f.last_commit()).unwrap();
        assert_eq!((outcome.restored, outcome.errors), (1, 0));
        assert_eq!(
            fs::read(f.root().join("_trash/a.1.jpg")).unwrap(),
            b"foreign trash"
        );
        let restored = f.rel_path_of(f.id_of("a.1.jpg"));
        assert_eq!(restored, "a.1.jpg");
        crate::scan::scan_project_inner(&f.db, f.root(), &mut |_| {}).unwrap();
        assert_eq!(f.status_of(f.id_of("a.1.jpg")), 0);
    }

    #[test]
    fn data_regression_undo_copy_preserves_replacement() {
        let f = project(&["a.jpg"]);
        enqueue(
            &f.db,
            f.targets("a.jpg"),
            ActionKind::Copy,
            Some("dest".into()),
            PairScope::Both,
        )
        .unwrap();
        f.commit();
        fs::remove_file(f.root().join("dest/a.jpg")).unwrap();
        fs::write(f.root().join("dest/a.jpg"), b"foreign replacement").unwrap();
        let outcome = undo_commit(&f.db, &f.store, f.last_commit()).unwrap();
        assert_eq!((outcome.restored, outcome.errors), (0, 1));
        assert_eq!(
            fs::read(f.root().join("dest/a.jpg")).unwrap(),
            b"foreign replacement"
        );
    }

    #[test]
    fn data_regression_delete_undo_restores_every_sidecar_endpoint() {
        let f = project(&["a.jpg"]);
        fs::write(f.root().join("a.xmp"), b"shared").unwrap();
        fs::write(f.root().join("a.jpg.xmp"), b"own").unwrap();
        enqueue(
            &f.db,
            f.targets("a.jpg"),
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        f.commit();
        let outcome = undo_commit(&f.db, &f.store, f.last_commit()).unwrap();
        assert_eq!((outcome.restored, outcome.errors), (1, 0));
        assert_eq!(fs::read(f.root().join("a.xmp")).unwrap(), b"shared");
        assert_eq!(fs::read(f.root().join("a.jpg.xmp")).unwrap(), b"own");
    }

    #[test]
    fn data_regression_undo_reports_nonreversible_entries_as_skipped() {
        let f = project(&["a.jpg"]);
        set_setting(&f.db, "deletionMode", "permanent").unwrap();
        enqueue(
            &f.db,
            f.targets("a.jpg"),
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        f.commit();
        let outcome = undo_commit(&f.db, &f.store, f.last_commit()).unwrap();
        assert_eq!((outcome.restored, outcome.skipped), (0, 1));
    }

    #[test]
    fn data_regression_large_history_preserves_summary_and_invalid_undo_semantics() {
        let f = project(&["a.jpg"]);
        f.db.call(|c| {
            c.execute_batch("INSERT INTO commits(id, started_at, status, summary) VALUES (1, 1, 1, '{}');
                WITH RECURSIVE numbers(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM numbers WHERE x < 16384)
                INSERT INTO commit_entries(commit_id, action, result, undo_info)
                SELECT 1, 1, 0, '{}' FROM numbers;
                INSERT INTO commit_entries(commit_id, action, result, undo_info) VALUES
                (1, 0, 0, '{\"mode\":\"trash\",\"trashPath\":\"_trash/a.jpg\"}'),
                (1, 0, 0, '{not json'),
                (1, 0, 0, '{\"mode\":\"permanent\"}'),
                (1, 0, 0, '{\"mode\":\"trash\",\"trashPath\":\"_trash/b.jpg\",\"wasPrimary\":\"invalid\"}'),
                (1, 2, 0, '{\"copyFingerprint\":\"recorded\"}'),
                (1, 2, 0, '{}'),
                (1, 3, 0, '{}'),
                (1, 1, 2, '{}');
                INSERT INTO commits(id, started_at, status, summary) VALUES (2, 2, 1, '{}');")?;
            Ok(())
        }).unwrap();
        let started = std::time::Instant::now();
        let summaries = list_commits(&f.db).unwrap();
        eprintln!(
            "History summary: 16392 entries, {} ms",
            started.elapsed().as_millis()
        );
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].id, 2);
        assert_eq!((summaries[0].moves, summaries[0].undoable), (0, 0));
        let summary = &summaries[1];
        assert_eq!(
            (summary.deletes, summary.moves, summary.copies, summary.xmp),
            (4, 16385, 2, 1)
        );
        assert_eq!((summary.errors, summary.undoable), (1, 16386));
    }

    #[test]
    fn a_pair_sidecar_travels_back_with_its_raw() {
        let f = project(&["IMG_1.cr3"]);
        set_rating(&f.db, f.targets("IMG_1.cr3"), 4).unwrap();
        f.commit();
        assert!(f.root().join("IMG_1.xmp").exists());

        enqueue(
            &f.db,
            f.targets("IMG_1.cr3"),
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        f.commit();
        assert!(!f.root().join("IMG_1.xmp").exists(), "sidecar went too");

        undo_commit(&f.db, &f.store, f.last_commit()).unwrap();
        assert!(f.root().join("IMG_1.cr3").exists());
        assert!(
            f.root().join("IMG_1.xmp").exists(),
            "the sidecar must come back with the photo"
        );
    }

    #[test]
    fn an_undone_entry_is_not_offered_again() {
        let f = project(&["a.jpg"]);
        enqueue(
            &f.db,
            f.targets("a.jpg"),
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        f.commit();
        let commit_id = f.last_commit();

        undo_commit(&f.db, &f.store, commit_id).unwrap();

        let summary = list_commits(&f.db)
            .unwrap()
            .into_iter()
            .find(|c| c.id == commit_id)
            .unwrap();
        assert_eq!(summary.undoable, 0);
        assert!(summary.undone_at.is_some(), "commit marked undone");
        assert_eq!(summary.deletes, 1, "the entry still shows in the history");

        // Running it a second time is a no-op, not a double restore.
        let again = undo_commit(&f.db, &f.store, commit_id).unwrap();
        assert_eq!((again.restored, again.errors), (0, 0));
    }
}
