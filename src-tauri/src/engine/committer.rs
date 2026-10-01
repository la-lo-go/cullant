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
///
/// In a RAW+JPEG pair both members map to the same `IMG.xmp`, so while they
/// agree the sidecar is written once and the dirty flag cleared for every
/// member. When they do NOT agree, no state is thrown away: the group's primary
/// keeps `IMG.xmp` — the name other applications look for — and the other half
/// is exported to its own `IMG.JPG.xmp`. Divergence is a legitimate way to work
/// (queue the RAWs, keep the JPEGs), so picking a winner would silently discard
/// a decision the user made on purpose.
struct DirtySidecar {
    /// Every dirty file mapping to this sidecar (all get `xmp_dirty` cleared).
    file_ids: Vec<i64>,
    /// rel_path of the representative photo.
    rel_path: String,
    /// The sidecar rel_path this write targets.
    sc_rel: String,
    /// rel_paths of every file merged into this sidecar. A file folded back into
    /// a shared sidecar may still have a per-file one on disk from when the pair
    /// disagreed; that copy is stale the moment it is represented here again.
    member_paths: Vec<String>,
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
        // The group's primary is visited first, so when a pair has diverged the
        // primary is the one that keeps the shared `IMG.xmp` and the other half
        // is the one pushed onto a per-file name. Ordering by id alone would
        // hand that role to whichever member the scan happened to insert first.
        let mut stmt = conn.prepare(
            "SELECT f.id, f.rel_path, f.rating, f.flag, f.label, f.orientation, f.xmp_dirty
             FROM files f JOIN groups g ON g.id = f.group_id
             WHERE f.status = 0 AND f.kind IN (0, 1)
             ORDER BY CASE WHEN f.id = g.primary_file_id THEN 0 ELSE 1 END, f.kind, f.id",
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
                r.get::<_, bool>(6)?,
            ))
        })?;
        let mut out: Vec<DirtySidecar> = Vec::new();
        let mut shared_paths = std::collections::HashMap::new();
        for row in rows {
            let (id, rel_path, state, dirty) = row?;
            let shared = xmp::sidecar_rel(&rel_path);
            let index = shared_paths.get(&shared).copied();
            match index.map(|index: usize| &mut out[index]) {
                // Same state: one sidecar speaks for both, exactly as before.
                Some(d) if d.state == state => {
                    if dirty {
                        d.file_ids.push(id);
                    }
                    d.member_paths.push(rel_path);
                }
                // Diverged: this half needs a sidecar of its own rather than
                // overwriting what the primary is about to export.
                Some(_) => {
                    let sc_rel = xmp::sidecar_rel_per_file(&rel_path);
                    out.push(DirtySidecar {
                        file_ids: if dirty { vec![id] } else { Vec::new() },
                        member_paths: vec![rel_path.clone()],
                        rel_path,
                        sc_rel,
                        state,
                    });
                }
                None => {
                    shared_paths.insert(shared.clone(), out.len());
                    out.push(DirtySidecar {
                        file_ids: if dirty { vec![id] } else { Vec::new() },
                        member_paths: vec![rel_path.clone()],
                        rel_path,
                        sc_rel: shared,
                        state,
                    });
                }
            }
        }
        Ok(out.into_iter().filter(|d| !d.file_ids.is_empty()).collect())
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
        let mut h = project_hasher(db);
        h.update(hash_pending(db, store, &deletes).as_bytes());
        h.update(format!("{deletion_mode:?}").as_bytes());
        format!("{:016x}", h.digest())
    };
    let moves_hash = hash_pending(db, store, &moves);
    let copies_hash = hash_pending(db, store, &copies);
    let xmp_hash = {
        let mut h = project_hasher(db);
        for d in &xmp {
            h.update(
                serde_json::json!([
                    d.file_ids,
                    d.sc_rel,
                    d.member_paths,
                    d.state.rating,
                    d.state.flag,
                    d.state.label,
                    d.state.orientation
                ])
                .to_string()
                .as_bytes(),
            );
        }
        format!("{:016x}", h.digest())
    };

    let mut hasher = project_hasher(db);
    for digest in [&deletes_hash, &moves_hash, &copies_hash, &xmp_hash] {
        hasher.update(digest.as_bytes());
    }
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

fn project_hasher(db: &Arc<Db>) -> Xxh3 {
    let mut hash = Xxh3::new();
    hash.update(
        serde_json::json!([db.project_root(), db.path(), db.generation()])
            .to_string()
            .as_bytes(),
    );
    hash
}

fn hash_pending(db: &Arc<Db>, store: &dyn ProjectStore, items: &[PendingAction]) -> String {
    let mut h = project_hasher(db);
    for p in items {
        let version = store
            .open_read(&p.rel_path)
            .and_then(|file| Ok(metadata_version(&file.metadata()?)))
            .unwrap_or_else(|error| format!("unavailable: {error}"));
        h.update(
            serde_json::json!([p.id, p.file_id, p.rel_path, p.action, p.dest, version])
                .to_string()
                .as_bytes(),
        );
    }
    format!("{:016x}", h.digest())
}

/// True when another still-present file (status = 0) shares this file's group,
/// and therefore its sidecar — pair members map to the same `IMG.xmp`. Used to
/// keep a shared sidecar alive when only one pair member is deleted. The file
/// being deleted is still status = 0 here, so `id != ?1` excludes it.
fn sidecar_shared_with_survivor(db: &Arc<Db>, file_id: i64) -> AppResult<bool> {
    db.call(move |conn| {
        let path: String = conn.query_row(
            "SELECT rel_path FROM files WHERE id = ?1",
            params![file_id],
            |r| r.get(0),
        )?;
        let shared = xmp::sidecar_rel(&path);
        let mut stmt = conn.prepare(
            "SELECT rel_path FROM files WHERE status = 0 AND id != ?1 AND kind IN (0, 1)
             AND dir = (SELECT dir FROM files WHERE id = ?1)",
        )?;
        let rows = stmt.query_map(params![file_id], |r| r.get::<_, String>(0))?;
        for row in rows {
            if xmp::sidecar_rel(&row?) == shared {
                return Ok(true);
            }
        }
        Ok(false)
    })
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SidecarChange {
    pub before_path: String,
    pub after_path: Option<String>,
}

/// What a commit entry needs in order to be reversed. Serialized into
/// `commit_entries.undo_info`.
///
/// Rows written before this struct existed carry only `mode`/`trashPath`; they
/// still deserialize, with the newer fields defaulting to "nothing to restore".
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct UndoInfo {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sidecars: Vec<SidecarChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_fingerprint: Option<String>,
    /// Deletes: "trash" (the file still exists) or "permanent" (it does not).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Deletes in trash mode: where the file now lives, project-relative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trash_path: Option<String>,
    /// Legacy deletes recorded one sidecar here. New entries use `sidecars`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidecar_trash_path: Option<String>,
    /// Legacy moves recorded this flag. New entries store each sidecar endpoint.
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

    /// Unreadable or absent undo info reads as "nothing recorded", which every
    /// caller already treats as not reversible — better than failing the whole
    /// history list over one malformed row.
    pub fn from_json(s: &str) -> UndoInfo {
        serde_json::from_str(s).unwrap_or_default()
    }
}

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
        unique = crate::store::collision_name(name, n);
        n += 1;
    }
    let destination = format!("{trash_parent}/{unique}");
    store.move_file(rel, &destination)?;
    Ok(destination)
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
        return Ok(());
    }
    run.begin_phase("deletes", deletes.len());
    for p in deletes {
        let mut undo = UndoInfo::default();
        let result = apply_delete(run, p, mode, &mut undo).map_err(|error| {
            let mut message = error.to_string();
            if let Some(trash) = &undo.trash_path {
                for sidecar in undo.sidecars.iter().rev() {
                    if let Some(after) = &sidecar.after_path {
                        if let Err(e) = run.store.move_file(after, &sidecar.before_path) {
                            message.push_str(&format!("; sidecar rollback failed: {e}"));
                        }
                    }
                }
                if let Err(e) = run.store.move_file(trash, &p.rel_path) {
                    message.push_str(&format!("; media rollback failed: {e}"));
                }
            } else if undo.mode.as_deref() == Some("permanent") {
                message.push_str("; media was deleted permanently before this error");
            }
            message
        });
        match &result {
            Ok(()) => run.note_ok(),
            Err(error) => run.note_err(&p.rel_path, error),
        }
        run.record(
            Some(p.file_id),
            0,
            Some(p.rel_path.clone()),
            None,
            Some(undo.to_json()),
            result,
        )?;
        run.tick();
    }
    Ok(())
}

fn apply_delete(
    run: &CommitRun,
    p: &PendingAction,
    mode: DeletionMode,
    undo: &mut UndoInfo,
) -> AppResult<()> {
    if !run.store.exists(&p.rel_path)? {
        return Err(AppError::Other("file missing on disk".into()));
    }
    let mut candidates = vec![xmp::sidecar_rel_per_file(&p.rel_path)];
    if !sidecar_shared_with_survivor(run.db, p.file_id)? {
        candidates.push(xmp::sidecar_rel(&p.rel_path));
    }
    let mut sidecars = Vec::new();
    for path in candidates {
        if run.store.exists(&path)? {
            sidecars.push(path);
        }
    }
    *undo = delete_via_store(run.store, &p.rel_path, mode)?;
    for before_path in sidecars {
        let sidecar = delete_via_store(run.store, &before_path, mode)?;
        undo.sidecar_trash_path = sidecar.trash_path.clone();
        undo.sidecars.push(SidecarChange {
            before_path,
            after_path: sidecar.trash_path,
        });
    }
    let file_id = p.file_id;
    undo.was_primary = run.db.call(move |conn| {
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
    let file_id = p.file_id;
    let source: String = run.db.call(move |conn| {
        Ok(conn.query_row(
            "SELECT rel_path FROM files WHERE id = ?1",
            params![file_id],
            |r| r.get(0),
        )?)
    })?;
    let dest_dir = p.dest.as_deref().unwrap_or_default();
    let (_, file_name) = split_parent(&source);
    let dest_rel = crate::store::join_relative(dest_dir, file_name);
    let mut undo = UndoInfo::default();
    let result: Result<(), String> = (|| {
        crate::store::validate_relative(dest_dir).map_err(|e| e.to_string())?;
        if !run.store.exists(&source).map_err(|e| e.to_string())? {
            return Err("file missing on disk".into());
        }
        check_move_copy_destination(run, file_id, &dest_rel).map_err(|e| e.to_string())?;
        run.store
            .create_dir_all(dest_dir)
            .map_err(|e| e.to_string())?;
        if action_i == 1 {
            apply_move(run, p, &source, &dest_rel, &mut undo)?;
        } else {
            apply_copy(run, p, &source, &dest_rel, &mut undo)?;
        }
        Ok(())
    })();
    match &result {
        Ok(()) => run.note_ok(),
        Err(e) => run.note_err(&source, e),
    }
    run.record(
        Some(p.file_id),
        action_i,
        Some(source),
        Some(dest_rel),
        Some(undo.to_json()),
        result,
    )?;
    run.tick();
    Ok(())
}

fn check_move_copy_destination(run: &CommitRun, file_id: i64, dest: &str) -> AppResult<()> {
    if run.store.exists(dest)? {
        return Err(AppError::Other(format!("target exists: {dest}")));
    }
    let dest = dest.to_string();
    let reserved = run.db.call(move |conn| {
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM files WHERE rel_path = ?1 AND id != ?2)",
            params![dest, file_id],
            |r| r.get::<_, bool>(0),
        )?)
    })?;
    if reserved {
        return Err(AppError::Other(
            "target path is reserved in the catalog".into(),
        ));
    }
    Ok(())
}

fn apply_move(
    run: &CommitRun,
    p: &PendingAction,
    source: &str,
    dest: &str,
    undo: &mut UndoInfo,
) -> Result<(), String> {
    let mut sidecars = vec![(
        xmp::sidecar_rel_per_file(source),
        xmp::sidecar_rel_per_file(dest),
    )];
    if !sidecar_shared_with_survivor(run.db, p.file_id).map_err(|e| e.to_string())? {
        sidecars.push((xmp::sidecar_rel(source), xmp::sidecar_rel(dest)));
    }
    let mut existing = Vec::new();
    for (from, to) in sidecars {
        if run.store.exists(&from).map_err(|e| e.to_string())? {
            if run.store.exists(&to).map_err(|e| e.to_string())? {
                return Err(format!("sidecar target exists: {to}"));
            }
            existing.push((from, to));
        }
    }
    run.store
        .move_file(source, dest)
        .map_err(|e| e.to_string())?;
    let applied: AppResult<()> = (|| {
        for (from, to) in existing {
            run.store.move_file(&from, &to)?;
            undo.sidecars.push(SidecarChange {
                before_path: from,
                after_path: Some(to),
            });
        }
        let (file_id, pending_id, dest) = (p.file_id, p.id, dest.to_string());
        run.db.call(move |conn| {
            let tx = conn.transaction()?;
            let (dir, _) = split_parent(&dest);
            tx.execute(
                "UPDATE files SET rel_path = ?2, dir = ?3 WHERE id = ?1",
                params![file_id, dest, dir],
            )?;
            tx.execute(
                "DELETE FROM pending_actions WHERE id = ?1",
                params![pending_id],
            )?;
            tx.commit()?;
            Ok(())
        })
    })();
    if let Err(error) = applied {
        let mut message = error.to_string();
        for sc in undo.sidecars.iter().rev() {
            if let Some(after) = &sc.after_path {
                if let Err(e) = run.store.move_file(after, &sc.before_path) {
                    message.push_str(&format!("; restore {} failed: {e}", sc.before_path));
                }
            }
        }
        if let Err(e) = run.store.move_file(dest, source) {
            message.push_str(&format!("; restore {source} failed: {e}"));
        }
        return Err(message);
    }
    undo.sidecar_moved = !undo.sidecars.is_empty();
    Ok(())
}

fn apply_copy(
    run: &CommitRun,
    p: &PendingAction,
    source: &str,
    dest: &str,
    undo: &mut UndoInfo,
) -> Result<(), String> {
    let staged = crate::store::temporary_sibling(run.store, dest).map_err(|e| e.to_string())?;
    let mut moved = false;
    let applied: AppResult<()> = (|| {
        run.store.copy(source, &staged)?;
        undo.copy_fingerprint = Some(file_fingerprint(run.store, &staged)?);
        run.store.move_file(&staged, dest)?;
        moved = true;
        let pending_id = p.id;
        run.db.call(move |conn| {
            conn.execute(
                "DELETE FROM pending_actions WHERE id = ?1",
                params![pending_id],
            )?;
            Ok(())
        })
    })();
    if let Err(error) = applied {
        let cleanup = if moved { dest } else { &staged };
        if run.store.exists(cleanup).map_err(|e| e.to_string())? {
            if moved
                && Some(file_fingerprint(run.store, dest).map_err(|e| e.to_string())?)
                    != undo.copy_fingerprint
            {
                return Err(format!("{error}; destination was replaced: {dest}"));
            }
            run.store
                .remove_file(cleanup)
                .map_err(|e| format!("{error}; partial copy remains at {cleanup}: {e}"))?;
        }
        return Err(error.to_string());
    }
    Ok(())
}

pub(crate) fn file_fingerprint(store: &dyn ProjectStore, rel: &str) -> AppResult<String> {
    use std::io::Read;
    let mut file = store.open_read(rel)?;
    let version = metadata_version(&file.metadata()?);
    let mut hash = Xxh3::new();
    hash.update(version.as_bytes());
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    if metadata_version(&file.metadata()?) != version {
        return Err(AppError::Other(format!(
            "file changed while reading: {rel}"
        )));
    }
    Ok(format!("{:016x}", hash.digest()))
}

fn metadata_version(metadata: &std::fs::Metadata) -> String {
    format!(
        "{}:{:?}:{:?}",
        metadata.len(),
        metadata.modified().ok(),
        metadata.created().ok()
    )
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
            xmp::write_sidecar(run.store, &d.sc_rel, &d.state).map_err(|e| e.to_string());
        match &result {
            Ok(sc_rel) => {
                run.note_ok();
                // A pair that has agreed again is back to one shared sidecar, so
                // the per-file copy written while it disagreed now describes a
                // state no file has. Left behind it would keep re-importing that
                // stale state on the next scan.
                for path in &d.member_paths {
                    let stale = xmp::sidecar_rel_per_file(path);
                    if stale != *sc_rel && run.store.exists(&stale).unwrap_or(false) {
                        if let Err(e) = run.store.remove_file(&stale) {
                            tracing::warn!("stale per-file sidecar {stale} not removed: {e}");
                        }
                    }
                }
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

    let phase_result: AppResult<()> = (|| {
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
        Ok(())
    })();
    if let Err(error) = phase_result {
        let phase = run.phase;
        run.note_err(phase, &error.to_string());
        if let Err(audit_error) =
            run.record(None, -1, None, None, None, Err(format!("{phase}: {error}")))
        {
            run.note_err("audit", &audit_error.to_string());
        }
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
            "pending actions changed since the preview. Review it again.".into(),
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
            "pending actions changed since the preview. Review it again.".into(),
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

        crate::engine::culling::set_rating(
            &db,
            Targets {
                ids: vec![ids(&db, "cr3")],
                as_groups: false,
            },
            5,
        )
        .unwrap();

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

        let outcome =
            execute_section(&db, &store, "deletes", &plan.deletes_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 0, "{:?}", outcome.error_samples);
        assert_eq!(outcome.ok, 1);
        assert!(!root.join("bad.jpg").exists(), "delete section ran");
        assert!(
            root.join("sel.jpg").exists(),
            "move section must NOT have run"
        );

        let plan2 = preview(&db, &store).unwrap();
        assert_eq!(plan2.deletes.len(), 0);
        assert_eq!(plan2.moves.len(), 1);
        assert_eq!(
            plan2.moves_hash, plan.moves_hash,
            "an untouched section's hash stays valid across a sibling commit"
        );

        assert!(execute_section(&db, &store, "moves", "deadbeef", |_, _, _| {}).is_err());

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
        assert_eq!(plan.xmp_count, 1);
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

    fn paired_project() -> (tempfile::TempDir, Arc<Db>, crate::store::LocalFsStore) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("IMG_1.cr3"), b"raw").unwrap();
        fs::write(root.join("IMG_1.jpg"), b"jpg").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        let store = crate::store::LocalFsStore::new(root);
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        (dir, db, store)
    }

    #[test]
    fn data_regression_trash_collision_preserves_source_names_and_media_extension() {
        let (dir, db, store) = paired_project();
        let root = dir.path();
        fs::create_dir(root.join("_trash")).unwrap();
        fs::write(root.join("_trash/IMG_1.jpg"), b"old trash").unwrap();
        fs::write(root.join("IMG_1.jpg.1"), b"foreign").unwrap();
        enqueue(
            &db,
            Targets {
                ids: vec![ids(&db, "jpg")],
                as_groups: false,
            },
            ActionKind::Delete,
            None,
            PairScope::Both,
        )
        .unwrap();
        let plan = preview(&db, &store).unwrap();
        let outcome =
            execute_section(&db, &store, "deletes", &plan.deletes_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 0);
        assert_eq!(fs::read(root.join("IMG_1.jpg.1")).unwrap(), b"foreign");
        assert_eq!(fs::read(root.join("_trash/IMG_1.1.jpg")).unwrap(), b"jpg");
    }

    #[test]
    fn data_regression_enqueue_rejects_destination_traversal() {
        let (_dir, db, _store) = paired_project();
        for dest in ["../escape", "..\\escape", "/absolute", "C:/absolute"] {
            let result = enqueue(
                &db,
                Targets {
                    ids: vec![ids(&db, "jpg")],
                    as_groups: false,
                },
                ActionKind::Move,
                Some(dest.into()),
                PairScope::Both,
            );
            assert!(result.is_err(), "accepted {dest}");
        }
        assert!(super::super::actions::list(&db).unwrap().is_empty());
    }

    fn queue_file(db: &Arc<Db>, rel: &str, action: ActionKind, dest: Option<&str>) {
        enqueue(
            db,
            Targets {
                ids: vec![id_of(db, rel)],
                as_groups: false,
            },
            action,
            dest.map(str::to_string),
            PairScope::Both,
        )
        .unwrap();
    }

    #[test]
    fn data_regression_move_reserved_db_path_keeps_source_and_finishes_history() {
        let (dir, db, store) = paired_project();
        fs::create_dir(dir.path().join("dest")).unwrap();
        fs::write(dir.path().join("dest/IMG_1.jpg"), b"reserved").unwrap();
        crate::scan::scan_project_inner(&db, dir.path(), &mut |_| {}).unwrap();
        fs::remove_file(dir.path().join("dest/IMG_1.jpg")).unwrap();
        queue_file(&db, "IMG_1.jpg", ActionKind::Move, Some("dest"));
        let plan = preview(&db, &store).unwrap();
        let outcome =
            execute_section(&db, &store, "moves", &plan.moves_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 1);
        assert!(dir.path().join("IMG_1.jpg").exists());
        assert!(!dir.path().join("dest/IMG_1.jpg").exists());
        let history = super::super::undo::list_commits(&db).unwrap();
        assert_eq!(history[0].status, 2);
        assert!(history[0].finished_at.is_some());
        assert_eq!(
            super::super::undo::commit_detail(&db, outcome.commit_id)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn data_regression_move_db_failure_rolls_back_and_finishes_history() {
        let (dir, db, store) = paired_project();
        queue_file(&db, "IMG_1.jpg", ActionKind::Move, Some("dest"));
        db.call(|c| { c.execute_batch("CREATE TRIGGER fail_move BEFORE UPDATE OF rel_path ON files BEGIN SELECT RAISE(ABORT, 'injected move failure'); END;")?; Ok(()) }).unwrap();
        let plan = preview(&db, &store).unwrap();
        let outcome =
            execute_section(&db, &store, "moves", &plan.moves_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 1);
        assert!(dir.path().join("IMG_1.jpg").exists());
        assert!(!dir.path().join("dest/IMG_1.jpg").exists());
        let history = super::super::undo::list_commits(&db).unwrap();
        assert_eq!(history[0].status, 2);
        assert!(history[0].finished_at.is_some());
    }

    #[test]
    fn data_regression_move_and_copy_use_the_current_source_path() {
        let (dir, db, store) = paired_project();
        queue_file(&db, "IMG_1.jpg", ActionKind::Move, Some("moved"));
        queue_file(&db, "IMG_1.jpg", ActionKind::Copy, Some("copied"));
        let plan = preview(&db, &store).unwrap();
        let outcome = execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 0, "{:?}", outcome.error_samples);
        assert_eq!(
            fs::read(dir.path().join("moved/IMG_1.jpg")).unwrap(),
            b"jpg"
        );
        assert_eq!(
            fs::read(dir.path().join("copied/IMG_1.jpg")).unwrap(),
            b"jpg"
        );
        assert!(super::super::actions::list(&db).unwrap().is_empty());
    }

    #[test]
    fn data_regression_sidecar_destination_collision_blocks_move() {
        let (dir, db, store) = paired_project();
        fs::create_dir(dir.path().join("dest")).unwrap();
        fs::write(dir.path().join("IMG_1.jpg.xmp"), b"own sidecar").unwrap();
        fs::write(dir.path().join("dest/IMG_1.jpg.xmp"), b"foreign sidecar").unwrap();
        queue_file(&db, "IMG_1.jpg", ActionKind::Move, Some("dest"));
        let plan = preview(&db, &store).unwrap();
        let outcome =
            execute_section(&db, &store, "moves", &plan.moves_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 1);
        assert!(dir.path().join("IMG_1.jpg").exists());
        assert_eq!(
            fs::read(dir.path().join("dest/IMG_1.jpg.xmp")).unwrap(),
            b"foreign sidecar"
        );
    }

    #[test]
    fn data_regression_divergent_jpeg_move_keeps_raw_shared_sidecar() {
        let (dir, db, store) = paired_project();
        fs::write(dir.path().join("IMG_1.xmp"), b"raw state").unwrap();
        fs::write(dir.path().join("IMG_1.jpg.xmp"), b"jpeg state").unwrap();
        queue_file(&db, "IMG_1.jpg", ActionKind::Move, Some("dest"));
        let plan = preview(&db, &store).unwrap();
        let outcome =
            execute_section(&db, &store, "moves", &plan.moves_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 0);
        assert_eq!(
            fs::read(dir.path().join("IMG_1.xmp")).unwrap(),
            b"raw state"
        );
        assert_eq!(
            fs::read(dir.path().join("dest/IMG_1.jpg.xmp")).unwrap(),
            b"jpeg state"
        );
        let undo = super::super::undo::undo_commit(&db, &store, outcome.commit_id).unwrap();
        assert_eq!((undo.restored, undo.errors), (1, 0));
        assert_eq!(
            fs::read(dir.path().join("IMG_1.jpg.xmp")).unwrap(),
            b"jpeg state"
        );
    }

    struct FaultStore(crate::store::LocalFsStore, u8);

    impl ProjectStore for FaultStore {
        fn list_recursive(
            &self,
            skip: &[&str],
            progress: &mut dyn FnMut(usize),
        ) -> AppResult<Vec<crate::store::StoreEntry>> {
            self.0.list_recursive(skip, progress)
        }
        fn open_read(&self, rel: &str) -> AppResult<std::fs::File> {
            self.0.open_read(rel)
        }
        fn open_write(&self, rel: &str, mime: &str) -> AppResult<std::fs::File> {
            self.0.open_write(rel, mime)
        }
        fn create_dir_all(&self, rel: &str) -> AppResult<()> {
            self.0.create_dir_all(rel)
        }
        fn rename_in_place(&self, rel: &str, name: &str) -> AppResult<String> {
            self.0.rename_in_place(rel, name)
        }
        fn move_to(&self, rel: &str, parent: &str) -> AppResult<String> {
            if self.1 == 0 && rel.ends_with(".xmp") {
                return Err(AppError::Other("injected sidecar move error".into()));
            }
            self.0.move_to(rel, parent)
        }
        fn copy(&self, from: &str, to: &str) -> AppResult<()> {
            if self.1 == 2 {
                fs::write(
                    self.0.local_path("dest/IMG_1.jpg").unwrap(),
                    b"concurrent foreign file",
                )?;
                return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).into());
            }
            self.0.copy(from, to)?;
            Err(AppError::Other("injected partial copy error".into()))
        }
        fn remove_file(&self, rel: &str) -> AppResult<()> {
            self.0.remove_file(rel)
        }
        fn exists(&self, rel: &str) -> AppResult<bool> {
            self.0.exists(rel)
        }
    }

    #[test]
    fn data_regression_copy_error_removes_partial_destination() {
        let (dir, db, local) = paired_project();
        let store = FaultStore(local, 1);
        queue_file(&db, "IMG_1.jpg", ActionKind::Copy, Some("dest"));
        let plan = preview(&db, &store).unwrap();
        let outcome =
            execute_section(&db, &store, "copies", &plan.copies_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 1);
        assert!(!dir.path().join("dest/IMG_1.jpg").exists());
        assert_eq!(super::super::actions::list(&db).unwrap().len(), 1);
    }

    #[test]
    fn data_regression_sidecar_move_error_keeps_media_and_queue() {
        let (dir, db, local) = paired_project();
        let store = FaultStore(local, 0);
        fs::write(dir.path().join("IMG_1.jpg.xmp"), b"sidecar").unwrap();
        queue_file(&db, "IMG_1.jpg", ActionKind::Move, Some("dest"));
        let plan = preview(&db, &store).unwrap();
        let outcome =
            execute_section(&db, &store, "moves", &plan.moves_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 1);
        assert!(dir.path().join("IMG_1.jpg").exists());
        assert!(dir.path().join("IMG_1.jpg.xmp").exists());
        assert_eq!(super::super::actions::list(&db).unwrap().len(), 1);
    }

    #[test]
    fn data_regression_reviewed_delete_binds_source_version() {
        let (dir, db, store) = paired_project();
        queue_file(&db, "IMG_1.jpg", ActionKind::Delete, None);
        let plan = preview(&db, &store).unwrap();
        fs::write(dir.path().join("IMG_1.jpg"), b"new unrelated photo").unwrap();
        assert!(execute_section(&db, &store, "deletes", &plan.deletes_hash, |_, _, _| {}).is_err());
        assert_eq!(
            fs::read(dir.path().join("IMG_1.jpg")).unwrap(),
            b"new unrelated photo"
        );
        assert!(super::super::undo::list_commits(&db).unwrap().is_empty());
    }

    #[test]
    fn data_regression_review_hash_binds_project_and_open_generation() {
        let (dir, db, store) = paired_project();
        let (other_dir, other_db, other_store) = paired_project();
        queue_file(&db, "IMG_1.jpg", ActionKind::Delete, None);
        queue_file(&other_db, "IMG_1.jpg", ActionKind::Delete, None);
        let plan = preview(&db, &store).unwrap();
        assert!(execute_section(
            &other_db,
            &other_store,
            "deletes",
            &plan.deletes_hash,
            |_, _, _| {}
        )
        .is_err());
        assert!(other_dir.path().join("IMG_1.jpg").exists());
        let reopened = Arc::new(Db::open(dir.path()).unwrap());
        assert!(execute_section(
            &reopened,
            &store,
            "deletes",
            &plan.deletes_hash,
            |_, _, _| {}
        )
        .is_err());
        assert!(dir.path().join("IMG_1.jpg").exists());
    }

    #[test]
    fn data_regression_reviewed_xmp_binds_the_exported_state() {
        let (_dir, db, store) = paired_project();
        rate(&db, &["jpg"], 2);
        let plan = preview(&db, &store).unwrap();
        rate(&db, &["jpg"], 5);
        assert!(execute_section(&db, &store, "xmp", &plan.xmp_hash, |_, _, _| {}).is_err());
    }

    #[test]
    fn data_regression_copy_error_preserves_a_concurrent_destination() {
        let (dir, db, local) = paired_project();
        let store = FaultStore(local, 2);
        queue_file(&db, "IMG_1.jpg", ActionKind::Copy, Some("dest"));
        let plan = preview(&db, &store).unwrap();
        let outcome =
            execute_section(&db, &store, "copies", &plan.copies_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 1);
        assert_eq!(
            fs::read(dir.path().join("dest/IMG_1.jpg")).unwrap(),
            b"concurrent foreign file"
        );
    }

    #[test]
    fn data_regression_delete_db_failure_restores_media_and_all_sidecars() {
        let (dir, db, store) = paired_project();
        queue_file(&db, "IMG_1.cr3", ActionKind::Delete, None);
        queue_file(&db, "IMG_1.jpg", ActionKind::Delete, None);
        fs::write(dir.path().join("IMG_1.jpg.xmp"), b"own").unwrap();
        fs::write(dir.path().join("IMG_1.xmp"), b"shared").unwrap();
        db.call(|c| { c.execute_batch("CREATE TRIGGER fail_delete BEFORE UPDATE OF status ON files BEGIN SELECT RAISE(ABORT, 'injected delete failure'); END;")?; Ok(()) }).unwrap();
        let plan = preview(&db, &store).unwrap();
        let outcome =
            execute_section(&db, &store, "deletes", &plan.deletes_hash, |_, _, _| {}).unwrap();
        assert_eq!((outcome.ok, outcome.errors), (0, 2));
        assert!(dir.path().join("IMG_1.cr3").exists());
        assert!(dir.path().join("IMG_1.jpg").exists());
        assert_eq!(fs::read(dir.path().join("IMG_1.jpg.xmp")).unwrap(), b"own");
        assert_eq!(fs::read(dir.path().join("IMG_1.xmp")).unwrap(), b"shared");
        let detail = super::super::undo::commit_detail(&db, outcome.commit_id).unwrap();
        assert_eq!(detail.len(), 2);
        assert!(detail
            .iter()
            .all(|e| e.action == 0 && e.file_id.is_some() && e.result == 2));
    }

    #[test]
    fn a_diverged_pair_writes_a_sidecar_each_instead_of_picking_a_winner() {
        let (dir, db, store) = paired_project();
        let root = dir.path();

        rate(&db, &["cr3"], 5);
        rate(&db, &["jpg"], 2);

        let plan = preview(&db, &store).unwrap();
        assert_eq!(plan.xmp_count, 2, "each half needs its own sidecar");
        let outcome = execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 0, "{:?}", outcome.error_samples);

        // The primary (the RAW) keeps the name other applications look for.
        let shared = fs::read_to_string(root.join("IMG_1.xmp")).unwrap();
        assert!(shared.contains("xmp:Rating=\"5\""), "{shared}");
        let own = fs::read_to_string(root.join("IMG_1.jpg.xmp")).unwrap();
        assert!(own.contains("xmp:Rating=\"2\""), "{own}");
    }

    #[test]
    fn data_regression_dirty_jpeg_does_not_replace_clean_raw_shared_state() {
        let (dir, db, store) = paired_project();
        rate(&db, &["cr3", "jpg"], 5);
        let plan = preview(&db, &store).unwrap();
        execute_section(&db, &store, "xmp", &plan.xmp_hash, |_, _, _| {}).unwrap();
        let original = fs::read(dir.path().join("IMG_1.xmp")).unwrap();
        rate(&db, &["jpg"], 2);
        let plan = preview(&db, &store).unwrap();
        execute_section(&db, &store, "xmp", &plan.xmp_hash, |_, _, _| {}).unwrap();
        assert_eq!(fs::read(dir.path().join("IMG_1.xmp")).unwrap(), original);
        let jpeg = fs::read_to_string(dir.path().join("IMG_1.jpg.xmp")).unwrap();
        assert_eq!(xmp::read_sidecar(&jpeg).unwrap().rating, 2);
    }

    #[test]
    fn agreeing_again_removes_the_stale_per_file_sidecar() {
        let (dir, db, store) = paired_project();
        let root = dir.path();

        rate(&db, &["cr3"], 5);
        rate(&db, &["jpg"], 2);
        let plan = preview(&db, &store).unwrap();
        execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();
        assert!(root.join("IMG_1.jpg.xmp").exists());

        // Settle the pair: one sidecar speaks for it again, so the per-file copy
        // now describes a state no file has and must not survive to be re-imported.
        crate::engine::groups::sync_state(
            &db,
            group_of(&db, "cr3"),
            crate::engine::groups::SyncFrom::Raw,
        )
        .unwrap();
        let plan = preview(&db, &store).unwrap();
        execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();

        assert!(
            !root.join("IMG_1.jpg.xmp").exists(),
            "the stale per-file sidecar must go when the pair agrees again"
        );
        let shared = fs::read_to_string(root.join("IMG_1.xmp")).unwrap();
        assert!(shared.contains("xmp:Rating=\"5\""), "{shared}");
    }

    #[test]
    fn deleting_a_diverged_half_takes_only_its_own_sidecar() {
        let (dir, db, store) = paired_project();
        let root = dir.path();
        set_setting(&db, "deletionMode", "permanent").unwrap();

        rate(&db, &["cr3"], 5);
        rate(&db, &["jpg"], 2);
        let plan = preview(&db, &store).unwrap();
        execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();

        enqueue(
            &db,
            Targets {
                ids: vec![ids(&db, "jpg")],
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

        assert!(
            !root.join("IMG_1.jpg.xmp").exists(),
            "a per-file sidecar names one file and goes with it"
        );
        assert!(
            root.join("IMG_1.xmp").exists(),
            "the surviving RAW's own sidecar must stay"
        );
    }

    fn group_of(db: &Arc<Db>, ext: &str) -> i64 {
        let ext = ext.to_string();
        db.call(move |c| {
            Ok(c.query_row(
                "SELECT group_id FROM files WHERE ext = ?1",
                params![ext],
                |r| r.get(0),
            )?)
        })
        .unwrap()
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

        rate(&db, &["cr3", "jpg"], 4);

        let plan = preview(&db, &store).unwrap();
        assert_eq!(plan.xmp_count, 1, "pair members share one sidecar");
        let outcome = execute(&db, &store, &plan.plan_hash, |_, _, _| {}).unwrap();
        assert_eq!(outcome.errors, 0, "{:?}", outcome.error_samples);
        assert_eq!(outcome.ok, 1, "one sidecar write, not two");
        let content = fs::read_to_string(root.join("IMG_1.xmp")).unwrap();
        assert!(content.contains("xmp:Rating=\"4\""));
        assert!(
            !root.join("IMG_1.jpg.xmp").exists(),
            "halves that agree need no per-file sidecar"
        );
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
