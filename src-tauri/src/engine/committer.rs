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
    Permanent,
    #[default]
    Trash, // project-local _trash folder (recoverable; same on desktop and mobile)
}

impl DeletionMode {
    pub fn from_setting(s: &str) -> DeletionMode {
        match s {
            "permanent" => DeletionMode::Permanent,
            // Unknown or removed values (e.g. a legacy "recycle") fall back to
            // the default: the project-local _trash folder.
            _ => DeletionMode::Trash,
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
    // Per-section digests. A single-section commit (the hold-to-run buttons)
    // validates only its own section against the current preview, so executing
    // one section never invalidates the others' hashes the way the whole-plan
    // `plan_hash` would.
    pub deletes_hash: String,
    pub moves_hash: String,
    pub copies_hash: String,
    pub xmp_hash: String,
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

/// One pending sidecar write: dirty photos grouped by their sidecar path.
/// In a RAW+JPEG pair both members map to the same `IMG.xmp`, so the sidecar
/// is written once and the dirty flag cleared for every member. Pair members
/// normally share rating/flag/label via group fan-out; if they have diverged,
/// the higher-id row wins (last writer).
struct DirtySidecar {
    /// Every dirty file mapping to this sidecar (all get `xmp_dirty` cleared).
    file_ids: Vec<i64>,
    /// rel_path of the representative photo (the winner when states diverge).
    rel_path: String,
    /// The sidecar rel_path all members map to (`IMG.CR3`/`IMG.JPG` → `IMG.xmp`).
    sc_rel: String,
    state: XmpState,
}

/// Count of photos with a pending XMP export. XMP dirtiness lives on the `files`
/// row (not the pending-actions queue), so this is the only way the toolbar can
/// tell the commit button there is XMP work waiting. Cheap: a single COUNT over
/// the (bounded) files table, run only on commit/classification events.
pub fn xmp_dirty_count(db: &Arc<Db>) -> AppResult<i64> {
    db.call_read(|conn| {
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM files
             WHERE status = 0 AND kind IN (0, 1) AND xmp_dirty = 1",
            [],
            |r| r.get(0),
        )?)
    })
}

/// Photos (kind 0 = RAW, 1 = image/JPEG) with pending XMP export, deduped by
/// sidecar path. Videos (kind 2) never get sidecars.
fn xmp_dirty_photos(db: &Arc<Db>) -> AppResult<Vec<DirtySidecar>> {
    db.call(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, rel_path, rating, flag, label, orientation FROM files
             WHERE status = 0 AND kind IN (0, 1) AND xmp_dirty = 1
             ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                XmpState {
                    rating: r.get::<_, i64>(2)?,
                    flag: r.get::<_, i64>(3)?,
                    label: r.get::<_, Option<String>>(4)?,
                    orientation: r.get::<_, Option<i64>>(5)?.unwrap_or(1),
                },
            ))
        })?;
        let mut out: Vec<DirtySidecar> = Vec::new();
        for row in rows {
            let (id, rel_path, state) = row?;
            let sc_rel = xmp::sidecar_rel(&rel_path);
            match out.iter_mut().find(|d| d.sc_rel == sc_rel) {
                Some(d) => {
                    d.file_ids.push(id);
                    d.rel_path = rel_path;
                    d.state = state;
                }
                None => out.push(DirtySidecar {
                    file_ids: vec![id],
                    rel_path,
                    sc_rel,
                    state,
                }),
            }
        }
        Ok(out)
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

    let xmp = xmp_dirty_photos(db)?;

    // Per-section digests. The delete section folds in the deletion mode (it
    // changes what the commit does), so switching mode re-previews and refreshes
    // the hold-to-run hash the dialog holds.
    let deletes_hash = {
        let mut h = Xxh3::new();
        for p in &deletes {
            h.update(format!("{}:{}", p.id, p.file_id).as_bytes());
        }
        h.update(format!("{deletion_mode:?}").as_bytes());
        format!("{:016x}", h.digest())
    };
    let moves_hash = hash_pending(&moves);
    let copies_hash = hash_pending(&copies);
    let xmp_hash = {
        let mut h = Xxh3::new();
        for d in &xmp {
            for id in &d.file_ids {
                h.update(format!("x{id}").as_bytes());
            }
        }
        format!("{:016x}", h.digest())
    };

    let mut hasher = Xxh3::new();
    for list in [&deletes, &moves, &copies] {
        for p in list {
            hasher.update(format!("{}:{}:{:?}", p.id, p.file_id, p.dest).as_bytes());
        }
    }
    for d in &xmp {
        for id in &d.file_ids {
            hasher.update(format!("x{id}").as_bytes());
        }
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
        deletes_hash,
        moves_hash,
        copies_hash,
        xmp_hash,
    })
}

/// Digest of a move/copy section: id, file id and destination of each action.
fn hash_pending(items: &[PendingAction]) -> String {
    let mut h = Xxh3::new();
    for p in items {
        h.update(format!("{}:{}:{:?}", p.id, p.file_id, p.dest).as_bytes());
    }
    format!("{:016x}", h.digest())
}

/// True when another still-present file (status = 0) shares this file's group,
/// and therefore its sidecar — pair members map to the same `IMG.xmp`. Used to
/// keep a shared sidecar alive when only one pair member is deleted. The file
/// being deleted is still status = 0 here, so `id != ?1` excludes it.
fn sidecar_shared_with_survivor(db: &Arc<Db>, file_id: i64) -> AppResult<bool> {
    db.call(move |conn| {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM files
             WHERE status = 0 AND id != ?1
               AND group_id = (SELECT group_id FROM files WHERE id = ?1)",
            params![file_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    })
}

/// What a commit entry needs in order to be reversed. Serialized into
/// `commit_entries.undo_info`.
///
/// Rows written before this struct existed carry only `mode`/`trashPath`; they
/// still deserialize, with the newer fields defaulting to "nothing to restore".
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct UndoInfo {
    /// Deletes: "trash" (the file still exists) or "permanent" (it does not).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Deletes in trash mode: where the file now lives, project-relative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trash_path: Option<String>,
    /// Deletes: where the photo's XMP sidecar was put, when it went too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidecar_trash_path: Option<String>,
    /// Moves: the sidecar travelled with the file. Both of its endpoints derive
    /// from the entry's own before/after paths, so no path is stored.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sidecar_moved: bool,
    /// Deletes: the file was its group's primary, and deleting it promoted a
    /// survivor. The old value is not recoverable from the row afterwards, so
    /// it is recorded here to be handed back on undo.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub was_primary: bool,
}

impl UndoInfo {
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Delete one file (by rel_path) according to the mode, through the store.
/// Returns the undo info describing where it went.
fn delete_via_store(
    store: &dyn ProjectStore,
    rel: &str,
    mode: DeletionMode,
) -> AppResult<UndoInfo> {
    match mode {
        DeletionMode::Permanent => {
            store.remove_file(rel)?;
            Ok(UndoInfo {
                mode: Some("permanent".into()),
                ..Default::default()
            })
        }
        DeletionMode::Trash => Ok(UndoInfo {
            mode: Some("trash".into()),
            trash_path: Some(store_trash(store, rel)?),
            ..Default::default()
        }),
    }
}

/// Move a file into the project-local `_trash` folder, preserving its relative
/// directory structure, with `.N` suffixing on collision. Returns the
/// project-relative path it ended up at.
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
    store.move_to(&src_rel, &trash_parent)
}

/// Mutable bookkeeping shared by the commit phases (deletes, moves, copies,
/// XMP): running counts, a globally capped sample of error strings, per-phase
/// progress ticks, and the `commit_entries` audit writer. Progress is reported
/// per phase (label + local done/total) so the dialog can name the current step
/// ("Deleting 5/12") instead of one opaque overall bar.
struct CommitRun<'a> {
    db: &'a Arc<Db>,
    store: &'a dyn ProjectStore,
    commit_id: i64,
    ok: usize,
    errors: usize,
    error_samples: Vec<String>,
    phase: &'static str,
    phase_done: usize,
    phase_total: usize,
    progress: &'a mut dyn FnMut(&str, usize, usize),
}

impl CommitRun<'_> {
    fn note_ok(&mut self) {
        self.ok += 1;
    }

    fn note_err(&mut self, ctx: &str, err: &str) {
        self.errors += 1;
        if self.error_samples.len() < 5 {
            self.error_samples.push(format!("{ctx}: {err}"));
        }
    }

    /// Enter a phase and emit its opening 0/total, so the UI shows the step
    /// immediately even before the first item finishes.
    fn begin_phase(&mut self, phase: &'static str, total: usize) {
        self.phase = phase;
        self.phase_done = 0;
        self.phase_total = total;
        (self.progress)(phase, 0, total);
    }

    fn tick(&mut self) {
        self.phase_done += 1;
        (self.progress)(self.phase, self.phase_done, self.phase_total);
    }

    /// Append one row to `commit_entries` describing what happened to a file.
    fn record(
        &self,
        file_id: Option<i64>,
        action: i64,
        before: Option<String>,
        after: Option<String>,
        undo: Option<String>,
        result: Result<(), String>,
    ) -> AppResult<()> {
        let commit_id = self.commit_id;
        let (res_i, err) = match &result {
            Ok(()) => (0i64, None),
            Err(e) => (2i64, Some(e.clone())),
        };
        self.db.call(move |conn| {
            conn.execute(
                "INSERT INTO commit_entries
                 (commit_id, file_id, action, before_path, after_path, undo_info, result, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![commit_id, file_id, action, before, after, undo, res_i, err],
            )?;
            Ok(())
        })
    }
}

fn run_deletes(
    run: &mut CommitRun,
    deletes: &[PendingAction],
    mode: DeletionMode,
) -> AppResult<()> {
    if deletes.is_empty() {
        return Ok(()); // no phase event for an empty section (avoids UI flicker)
    }
    run.begin_phase("deletes", deletes.len());
    for p in deletes {
        let mut result: Result<UndoInfo, String> = match run.store.exists(&p.rel_path) {
            Ok(true) => delete_via_store(run.store, &p.rel_path, mode).map_err(|e| e.to_string()),
            Ok(false) => Err("file missing on disk".into()),
            // A real access error (e.g. a disconnected network project folder)
            // must not masquerade as "file missing on disk".
            Err(e) => Err(format!("cannot access file: {e}")),
        };
        // A photo's sidecar travels with it — but a RAW+JPEG pair shares one
        // sidecar (IMG.CR3 and IMG.JPG both map to IMG.xmp). Deleting only one
        // member (delete-RAW-only, or a per-member reject) must NOT remove the
        // sidecar while the partner survives, or that survivor loses its
        // exported rating/flag/label. Remove it only when no live file maps to it.
        if let Ok(undo) = &mut result {
            let sidecar_rel = xmp::sidecar_rel(&p.rel_path);
            if sidecar_rel != p.rel_path
                && run.store.exists(&sidecar_rel).unwrap_or(false)
                && !sidecar_shared_with_survivor(run.db, p.file_id)?
            {
                match delete_via_store(run.store, &sidecar_rel, mode) {
                    // Recorded on the photo's own entry rather than as a row of
                    // its own, so undoing the photo brings its sidecar back in
                    // the same step.
                    Ok(sc) => undo.sidecar_trash_path = sc.trash_path,
                    Err(e) => tracing::warn!("sidecar delete failed: {e}"),
                }
            }
        }
        match &mut result {
            Ok(undo) => {
                run.note_ok();
                let file_id = p.file_id;
                // Promoting a survivor overwrites `primary_file_id`, and the old
                // value is exactly this file — but only when the UPDATE actually
                // matched. Report that back so undo can hand the role over again.
                let was_primary = run.db.call(move |conn| {
                    let tx = conn.transaction()?;
                    tx.execute(
                        "UPDATE files SET status = 2 WHERE id = ?1",
                        params![file_id],
                    )?;
                    tx.execute(
                        "DELETE FROM pending_actions WHERE file_id = ?1",
                        params![file_id],
                    )?;
                    let promoted = tx.execute(
                        "UPDATE groups SET primary_file_id =
                           (SELECT id FROM files WHERE group_id = groups.id AND status = 0 LIMIT 1)
                         WHERE primary_file_id = ?1",
                        params![file_id],
                    )?;
                    tx.commit()?;
                    Ok(promoted > 0)
                })?;
                undo.was_primary = was_primary;
                run.record(
                    Some(p.file_id),
                    0,
                    Some(p.rel_path.clone()),
                    None,
                    Some(undo.to_json()),
                    Ok(()),
                )?;
            }
            Err(e) => {
                run.note_err(&p.rel_path, e);
                run.record(
                    Some(p.file_id),
                    0,
                    Some(p.rel_path.clone()),
                    None,
                    None,
                    Err(e.clone()),
                )?;
            }
        }
        run.tick();
    }
    Ok(())
}

fn run_moves(run: &mut CommitRun, moves: &[PendingAction]) -> AppResult<()> {
    if moves.is_empty() {
        return Ok(());
    }
    run.begin_phase("moves", moves.len());
    for p in moves {
        process_move_copy(run, p, 1)?;
    }
    Ok(())
}

fn run_copies(run: &mut CommitRun, copies: &[PendingAction]) -> AppResult<()> {
    if copies.is_empty() {
        return Ok(());
    }
    run.begin_phase("copies", copies.len());
    for p in copies {
        process_move_copy(run, p, 2)?;
    }
    Ok(())
}

/// One move (`action_i == 1`) or copy (`action_i == 2`), including its sidecar,
/// DB row update and audit record. Ticks the current phase once.
fn process_move_copy(run: &mut CommitRun, p: &PendingAction, action_i: i64) -> AppResult<()> {
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

    let mut undo = UndoInfo::default();
    let result: Result<(), String> = (|| {
        if !run.store.exists(&p.rel_path).map_err(|e| e.to_string())? {
            return Err("file missing on disk".into());
        }
        if run.store.exists(&dest_rel).map_err(|e| e.to_string())? {
            return Err(format!("target exists: {dest_rel}"));
        }
        run.store
            .create_dir_all(&dest_dir)
            .map_err(|e| e.to_string())?;
        if action_i == 1 {
            run.store
                .move_to(&p.rel_path, &dest_dir)
                .map_err(|e| e.to_string())?;
        } else {
            run.store
                .copy(&p.rel_path, &dest_rel)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();

    match &result {
        Ok(()) => {
            run.note_ok();
            // A move carries the file's sidecar along (a copy leaves it
            // behind). Same collision policy as the file itself: the
            // target must not exist, and a sidecar failure only warns.
            if action_i == 1 {
                let sidecar_rel = xmp::sidecar_rel(&p.rel_path);
                if sidecar_rel != p.rel_path && run.store.exists(&sidecar_rel).unwrap_or(false) {
                    match run.store.move_to(&sidecar_rel, &dest_dir) {
                        // Both endpoints derive from the entry's own paths, so
                        // undo only needs to know that it happened.
                        Ok(_) => undo.sidecar_moved = true,
                        Err(e) => tracing::warn!("sidecar move failed: {e}"),
                    }
                }
            }
            let file_id = p.file_id;
            let pending_id = p.id;
            if action_i == 1 {
                let new_rel = dest_rel.clone();
                // One transaction so a crash cannot leave the new rel_path
                // recorded while the stale pending move row survives (which
                // would resurface as a bogus pending action next preview).
                run.db.call(move |conn| {
                    let tx = conn.transaction()?;
                    let dir = new_rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                    tx.execute(
                        "UPDATE files SET rel_path = ?2, dir = ?3 WHERE id = ?1",
                        params![file_id, new_rel, dir],
                    )?;
                    tx.execute(
                        "DELETE FROM pending_actions WHERE id = ?1",
                        params![pending_id],
                    )?;
                    tx.commit()?;
                    Ok(())
                })?;
            } else {
                run.db.call(move |conn| {
                    conn.execute(
                        "DELETE FROM pending_actions WHERE id = ?1",
                        params![pending_id],
                    )?;
                    Ok(())
                })?;
            }
        }
        Err(e) => {
            run.note_err(&p.rel_path, e);
        }
    }
    let undo_json = result.is_ok().then(|| undo.to_json());
    run.record(
        Some(p.file_id),
        action_i,
        Some(p.rel_path.clone()),
        Some(dest_rel),
        undo_json,
        result,
    )?;
    run.tick();
    Ok(())
}

/// Write pending XMP sidecars. Resolved only now, AFTER deletes and moves/copies:
/// deleted files are status = 2 (excluded by the query, so no orphan sidecar is
/// written at their old path) and moved files come back with their new rel_path.
fn run_xmp(run: &mut CommitRun) -> AppResult<()> {
    let xmp_files = xmp_dirty_photos(run.db)?;
    if xmp_files.is_empty() {
        return Ok(());
    }
    run.begin_phase("xmp", xmp_files.len());
    for d in xmp_files {
        let result: Result<String, String> =
            xmp::write_sidecar(run.store, &d.rel_path, &d.state).map_err(|e| e.to_string());
        match &result {
            Ok(sc_rel) => {
                run.note_ok();
                // Every file mapping to this sidecar (a RAW+JPEG pair shares it)
                // is now exported.
                let file_ids = d.file_ids.clone();
                run.db.call(move |conn| {
                    for id in &file_ids {
                        conn.execute("UPDATE files SET xmp_dirty = 0 WHERE id = ?1", params![id])?;
                    }
                    Ok(())
                })?;
                run.record(
                    Some(d.file_ids[0]),
                    3,
                    Some(d.rel_path.clone()),
                    Some(sc_rel.clone()),
                    None,
                    Ok(()),
                )?;
            }
            Err(e) => {
                run.note_err(&d.rel_path, e);
                run.record(
                    Some(d.file_ids[0]),
                    3,
                    Some(d.rel_path.clone()),
                    None,
                    None,
                    Err(e.clone()),
                )?;
            }
        }
        run.tick();
    }
    Ok(())
}

/// Which action sections a single execution runs. The whole-plan Execute runs
/// all four; each hold-to-run button runs exactly one.
#[derive(Clone, Copy)]
struct Phases {
    deletes: bool,
    moves: bool,
    copies: bool,
    xmp: bool,
}

/// Run the selected phases of an already-validated plan under one `commits`
/// audit row. Phases always run in the fixed order deletes → moves → copies →
/// XMP; XMP is resolved last (inside `run_xmp`) so it sees the post-move paths
/// and skips just-deleted files.
fn run_commit(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    plan: &CommitPlan,
    phases: Phases,
    mut progress: impl FnMut(&str, usize, usize),
) -> AppResult<CommitOutcome> {
    let started = now_secs();
    let summary = serde_json::json!({
        "deletes": if phases.deletes { plan.deletes.len() } else { 0 },
        "moves": if phases.moves { plan.moves.len() } else { 0 },
        "copies": if phases.copies { plan.copies.len() } else { 0 },
        "xmp": if phases.xmp { plan.xmp_count } else { 0 },
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

    let mut run = CommitRun {
        db,
        store,
        commit_id,
        ok: 0,
        errors: 0,
        error_samples: Vec::new(),
        phase: "",
        phase_done: 0,
        phase_total: 0,
        progress: &mut progress,
    };

    if phases.deletes {
        run_deletes(&mut run, &plan.deletes, plan.deletion_mode)?;
    }
    if phases.moves {
        run_moves(&mut run, &plan.moves)?;
    }
    if phases.copies {
        run_copies(&mut run, &plan.copies)?;
    }
    if phases.xmp {
        run_xmp(&mut run)?;
    }

    let CommitRun {
        ok,
        errors,
        error_samples,
        ..
    } = run;

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

pub fn execute(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    plan_hash: &str,
    progress: impl FnMut(&str, usize, usize),
) -> AppResult<CommitOutcome> {
    let plan = preview(db, store)?;
    if plan.plan_hash != plan_hash {
        return Err(AppError::Other(
            "pending actions changed since the preview — review again".into(),
        ));
    }
    run_commit(
        db,
        store,
        &plan,
        Phases {
            deletes: true,
            moves: true,
            copies: true,
            xmp: true,
        },
        progress,
    )
}

/// Execute a single section ("deletes" | "moves" | "copies" | "xmp"), validating
/// only that section's digest against a fresh preview. Powers the dialog's
/// per-section hold-to-run buttons, so committing one section leaves the others
/// pending with their own (still-valid) hashes.
pub fn execute_section(
    db: &Arc<Db>,
    store: &dyn ProjectStore,
    section: &str,
    section_hash: &str,
    progress: impl FnMut(&str, usize, usize),
) -> AppResult<CommitOutcome> {
    let plan = preview(db, store)?;
    let (current, phases) = match section {
        "deletes" => (
            &plan.deletes_hash,
            Phases {
                deletes: true,
                moves: false,
                copies: false,
                xmp: false,
            },
        ),
        "moves" => (
            &plan.moves_hash,
            Phases {
                deletes: false,
                moves: true,
                copies: false,
                xmp: false,
            },
        ),
        "copies" => (
            &plan.copies_hash,
            Phases {
                deletes: false,
                moves: false,
                copies: true,
                xmp: false,
            },
        ),
        "xmp" => (
            &plan.xmp_hash,
            Phases {
                deletes: false,
                moves: false,
                copies: false,
                xmp: true,
            },
        ),
        other => return Err(AppError::Other(format!("unknown commit section: {other}"))),
    };
    if current != section_hash {
        return Err(AppError::Other(
            "pending actions changed since the preview — review again".into(),
        ));
    }
    run_commit(db, store, &plan, phases, progress)
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

        let outcome = execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();
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
        assert!(execute(&db, &store, &plan.plan_hash, |_, _, _| {}).is_err());
    }

    fn rate(db: &Arc<Db>, exts: &[&str], rating: i64) {
        crate::engine::culling::set_rating(
            db,
            Targets {
                ids: exts.iter().map(|e| ids(db, e)).collect(),
                as_groups: false,
            },
            rating,
        )
        .unwrap();
    }

    fn id_of(db: &Arc<Db>, rel: &str) -> i64 {
        let rel = rel.to_string();
        db.call(move |c| {
            Ok(c.query_row(
                "SELECT id FROM files WHERE rel_path = ?1",
                params![rel],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    }

    #[test]
    fn section_commit_runs_one_section_and_leaves_the_rest_pending() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("bad.jpg"), b"jpg").unwrap();
        fs::write(root.join("sel.jpg"), b"jpg").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        set_setting(&db, "deletionMode", "trash").unwrap();

        enqueue(
            &db,
            Targets {
                ids: vec![id_of(&db, "bad.jpg")],
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
                ids: vec![id_of(&db, "sel.jpg")],
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

        // Commit ONLY the deletes section.
        let outcome =
            execute_section(&db, &store, "deletes", &plan.deletes_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 0, "{:?}", outcome.error_samples);
        assert_eq!(outcome.ok, 1);
        assert!(!root.join("bad.jpg").exists(), "delete section ran");
        assert!(
            root.join("sel.jpg").exists(),
            "move section must NOT have run"
        );

        // The move is still pending, and its hash is unchanged by the delete commit.
        let plan2 = preview(&db, &store).unwrap();
        assert_eq!(plan2.deletes.len(), 0);
        assert_eq!(plan2.moves.len(), 1);
        assert_eq!(
            plan2.moves_hash, plan.moves_hash,
            "an untouched section's hash stays valid across a sibling commit"
        );

        // A stale section hash is rejected.
        assert!(execute_section(&db, &store, "moves", "deadbeef", |_, _, _| {}).is_err());

        // Now commit the move section.
        let outcome =
            execute_section(&db, &store, "moves", &plan2.moves_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 0, "{:?}", outcome.error_samples);
        assert!(root.join("selects").join("sel.jpg").exists());
    }

    #[test]
    fn jpeg_gets_a_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("shot.jpg"), b"jpg").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();

        rate(&db, &["jpg"], 3);

        let plan = preview(&db, &store).unwrap();
        assert_eq!(plan.xmp_count, 1);
        let outcome = execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 0, "{:?}", outcome.error_samples);
        let content = fs::read_to_string(root.join("shot.xmp")).unwrap();
        assert!(content.contains("xmp:Rating=\"3\""));
    }

    #[test]
    fn deleted_photo_leaves_no_orphan_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("bad.cr3"), b"raw").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        set_setting(&db, "deletionMode", "trash").unwrap();

        // Dirty, then deleted in the same commit: no sidecar may be written at
        // the old (now empty) location, nor trashed alongside the file.
        rate(&db, &["cr3"], 1);
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

        let plan = preview(&db, &store).unwrap();
        assert_eq!(plan.xmp_count, 1); // still dirty at preview time
        let outcome = execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 0, "{:?}", outcome.error_samples);
        assert!(!root.join("bad.cr3").exists());
        assert!(
            !root.join("bad.xmp").exists(),
            "orphan sidecar at the old path"
        );
        assert!(
            !root.join("_trash").join("bad.xmp").exists(),
            "sidecar written then trashed"
        );
    }

    #[test]
    fn moved_photo_carries_its_sidecar_to_the_new_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("keep.cr3"), b"raw").unwrap();
        // Pre-existing sidecar with foreign content (Lightroom develop settings).
        fs::write(
            root.join("keep.xmp"),
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    crs:Exposure2012="+0.55"/>
 </rdf:RDF>
</x:xmpmeta>"#,
        )
        .unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();

        rate(&db, &["cr3"], 5);
        enqueue(
            &db,
            Targets {
                ids: vec![ids(&db, "cr3")],
                as_groups: false,
            },
            ActionKind::Move,
            Some("selects".into()),
            PairScope::Both,
        )
        .unwrap();

        let plan = preview(&db, &store).unwrap();
        let outcome = execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 0, "{:?}", outcome.error_samples);
        assert!(root.join("selects").join("keep.cr3").exists());
        assert!(
            !root.join("keep.xmp").exists(),
            "sidecar left at the old path"
        );
        // The XMP phase ran against the NEW rel_path and merged into the
        // carried sidecar, preserving the foreign content.
        let content = fs::read_to_string(root.join("selects").join("keep.xmp")).unwrap();
        assert!(content.contains("xmp:Rating=\"5\""));
        assert!(content.contains("crs:Exposure2012=\"+0.55\""));
    }

    #[test]
    fn deleting_one_pair_member_keeps_the_shared_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("IMG_1.cr3"), b"raw").unwrap();
        fs::write(root.join("IMG_1.jpg"), b"jpg").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        set_setting(&db, "deletionMode", "trash").unwrap();

        // Rate the pair and commit, so IMG_1.xmp exists and both members are clean.
        rate(&db, &["cr3", "jpg"], 5);
        let plan = preview(&db, &store).unwrap();
        execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();
        assert!(root.join("IMG_1.xmp").exists());

        // Delete the RAW only; the JPEG survives and still needs its sidecar.
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
        let plan = preview(&db, &store).unwrap();
        let outcome = execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 0, "{:?}", outcome.error_samples);

        assert!(!root.join("IMG_1.cr3").exists(), "RAW should be trashed");
        assert!(root.join("IMG_1.jpg").exists(), "JPEG must survive");
        assert!(
            root.join("IMG_1.xmp").exists(),
            "the surviving JPEG's shared sidecar must not be deleted with the RAW"
        );
    }

    #[test]
    fn undo_info_records_the_sidecar_and_the_lost_primary() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("IMG_9.cr3"), b"raw").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        set_setting(&db, "deletionMode", "trash").unwrap();

        // Rate then commit so a sidecar exists next to the RAW.
        rate(&db, &["cr3"], 3);
        let plan = preview(&db, &store).unwrap();
        execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();
        assert!(root.join("IMG_9.xmp").exists());

        let raw_id = ids(&db, "cr3");
        enqueue(
            &db,
            Targets {
                ids: vec![raw_id],
                as_groups: false,
            },
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        let plan = preview(&db, &store).unwrap();
        execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();

        let raw_undo: String = db
            .call(move |c| {
                Ok(c.query_row(
                    "SELECT undo_info FROM commit_entries WHERE action = 0 AND file_id = ?1",
                    params![raw_id],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        let undo: UndoInfo = serde_json::from_str(&raw_undo).unwrap();

        assert_eq!(undo.mode.as_deref(), Some("trash"));
        assert!(undo.trash_path.is_some(), "the file's own trash path");
        // The sidecar went to the trash too, and the entry says where — without
        // this an undo would restore the photo and strand its sidecar.
        let sc = undo
            .sidecar_trash_path
            .expect("sidecar trash path must be recorded");
        assert!(root.join(&sc).exists(), "sidecar really is at {sc}");
        // The RAW was its singleton group's primary, so the promotion fired and
        // overwrote the only record of that fact.
        assert!(undo.was_primary);
    }

    #[test]
    fn raw_jpeg_pair_writes_one_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("IMG_1.cr3"), b"raw").unwrap();
        fs::write(root.join("IMG_1.jpg"), b"jpg").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();

        // Both members dirty (mirrors what group fan-out produces).
        rate(&db, &["cr3", "jpg"], 4);

        let plan = preview(&db, &store).unwrap();
        assert_eq!(plan.xmp_count, 1, "pair members share one sidecar");
        let outcome = execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 0, "{:?}", outcome.error_samples);
        assert_eq!(outcome.ok, 1, "one sidecar write, not two");
        let content = fs::read_to_string(root.join("IMG_1.xmp")).unwrap();
        assert!(content.contains("xmp:Rating=\"4\""));
        // Both members got their dirty flag cleared by the single write.
        let dirty: i64 = db
            .call(|c| {
                Ok(
                    c.query_row("SELECT COUNT(*) FROM files WHERE xmp_dirty = 1", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(dirty, 0);
    }
}
