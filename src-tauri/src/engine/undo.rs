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

use super::committer::UndoInfo;
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
            Some("This entry failed, so nothing happened.".into()),
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
        ACTION_MOVE | ACTION_COPY => (true, None),
        ACTION_XMP => (
            false,
            Some("Sidecar writes merge in place. The previous contents were not kept.".into()),
        ),
        _ => (false, Some("Unknown action.".into())),
    }
}

pub fn list_commits(db: &Arc<Db>) -> AppResult<Vec<CommitSummary>> {
    let rows = db.call_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT c.id, c.started_at, c.finished_at, c.status, c.undone_at,
                    e.id, e.action, e.result, e.undone_at, e.undo_info
             FROM commits c
             LEFT JOIN commit_entries e ON e.commit_id = c.id
             ORDER BY c.id DESC, e.id ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, Option<i64>>(7)?,
                r.get::<_, Option<i64>>(8)?,
                r.get::<_, Option<String>>(9)?,
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;

    let mut out: Vec<CommitSummary> = Vec::new();
    for (id, started, finished, status, undone, entry_id, action, result, e_undone, undo_json) in
        rows
    {
        if out.last().map(|c| c.id) != Some(id) {
            out.push(CommitSummary {
                id,
                started_at: started,
                finished_at: finished,
                status,
                deletes: 0,
                moves: 0,
                copies: 0,
                xmp: 0,
                errors: 0,
                undoable: 0,
                undone_at: undone,
            });
        }
        // A commit with no entries at all (LEFT JOIN) still gets its summary row.
        let Some(_) = entry_id else { continue };
        let summary = out.last_mut().expect("pushed above");
        let action = action.unwrap_or(-1);
        let result = result.unwrap_or(0);
        match action {
            ACTION_DELETE => summary.deletes += 1,
            ACTION_MOVE => summary.moves += 1,
            ACTION_COPY => summary.copies += 1,
            ACTION_XMP => summary.xmp += 1,
            _ => {}
        }
        if result != 0 {
            summary.errors += 1;
        }
        let undo = UndoInfo::from_json(undo_json.as_deref().unwrap_or("{}"));
        if reversibility(action, result, e_undone, &undo).0 {
            summary.undoable += 1;
        }
    }
    Ok(out)
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
    // Walk the commit backwards: XMP, copies, moves, deletes. A move that
    // followed a delete must be reversed before the delete puts a file back at
    // the path the move vacated.
    items.sort_by_key(|r| (-r.action, -r.id));
    run(db, store, items)
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
fn restore_to(store: &dyn ProjectStore, from: &str, to: &str) -> AppResult<String> {
    let (parent, name) = split_parent(to);
    store.create_dir_all(parent)?;

    // Never overwrite whatever occupies the old path now — the point of an undo
    // is to lose nothing. Same `.N` suffixing the trash uses on collision.
    let mut unique = name.to_string();
    let mut n = 1;
    while store.exists(&join_rel(parent, &unique))? {
        unique = format!("{name}.{n}");
        n += 1;
    }
    let src = if unique != name {
        store.rename_in_place(from, &unique)?
    } else {
        from.to_string()
    };
    store.move_to(&src, parent)
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
    let restored = restore_to(store, trash_path, original)?;

    // The sidecar rides back with the photo. Best-effort: a photo restored
    // without its sidecar is still a restored photo, and the sidecar can be
    // rewritten from the database at the next commit.
    if let Some(sc_trash) = item.undo.sidecar_trash_path.as_deref() {
        let sc_target = xmp::sidecar_rel(&restored);
        if store.exists(sc_trash).unwrap_or(false) {
            if let Err(e) = restore_to(store, sc_trash, &sc_target) {
                tracing::warn!("sidecar restore failed: {e}");
            }
        }
    }

    let Some(file_id) = item.file_id else {
        return Ok(());
    };
    let dir = restored.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let (dir, restored_owned) = (dir.to_string(), restored.clone());
    let was_primary = item.undo.was_primary;
    db.call(move |conn| {
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE files SET status = 0, rel_path = ?2, dir = ?3 WHERE id = ?1",
            params![file_id, restored_owned, dir],
        )?;
        if was_primary {
            tx.execute(
                "UPDATE groups SET primary_file_id = ?2
                 WHERE id = (SELECT group_id FROM files WHERE id = ?1)",
                params![file_id, file_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    })
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
    let restored = restore_to(store, current, original)?;

    if item.undo.sidecar_moved {
        let sc_from = xmp::sidecar_rel(current);
        let sc_to = xmp::sidecar_rel(&restored);
        if store.exists(&sc_from).unwrap_or(false) {
            if let Err(e) = restore_to(store, &sc_from, &sc_to) {
                tracing::warn!("sidecar restore failed: {e}");
            }
        }
    }

    let Some(file_id) = item.file_id else {
        return Ok(());
    };
    let dir = restored.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let (dir, restored_owned) = (dir.to_string(), restored.clone());
    db.call(move |conn| {
        conn.execute(
            "UPDATE files SET rel_path = ?2, dir = ?3 WHERE id = ?1",
            params![file_id, restored_owned, dir],
        )?;
        Ok(())
    })
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
            f.root().join("a.jpg.1").exists(),
            "the restored file lands beside it"
        );
    }

    #[test]
    fn a_pair_sidecar_travels_back_with_its_raw() {
        let f = project(&["IMG_1.cr3"]);
        // Rate and commit so IMG_1.xmp exists next to the RAW.
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
