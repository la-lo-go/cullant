//! Copy of the live project database into the project folder, for stores that
//! cannot keep the live database there (Android SAF).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::store::ProjectStore;

const COPY_REL: &str = ".cullant/cullant.db";
const THUMBS_REL: &str = ".cullant/thumbs";
const WAL_REL: &str = ".cullant/cullant.db-wal";
const SNAPSHOT: &str = "mirror.tmp.db";
const RECORD: &str = "mirror.json";

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportOutcome {
    Written,
    Unchanged,
    /// The folder holds a copy this device did not write. Nothing was changed.
    Conflict,
}

/// Size and mtime in seconds of the folder copy and of its WAL. A desktop
/// that has the folder open keeps its latest edits in the WAL only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub size: i64,
    pub mtime: i64,
    /// `None` also for an empty WAL, which holds no data.
    pub wal: Option<(i64, i64)>,
}

/// What this device last wrote to the folder, imported or accepted.
#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Record {
    hash: u64,
    size: i64,
    mtime: i64,
    // Files from before the WAL check have no such field.
    #[serde(default)]
    wal: Option<(i64, i64)>,
}

impl Record {
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    fn new(hash: u64, fp: Fingerprint) -> Self {
        Record {
            hash,
            size: fp.size,
            mtime: fp.mtime,
            wal: fp.wal,
        }
    }
}

/// An empty WAL holds no data, so it counts as absent.
pub fn wal_fingerprint(stat: Option<(i64, i64)>) -> Option<(i64, i64)> {
    stat.filter(|&(size, _)| size > 0)
}

pub fn export_folder_copy(
    db: &Db,
    store: &dyn ProjectStore,
    base: &Path,
) -> AppResult<ExportOutcome> {
    if !store.mirrors_metadata() {
        return Err(AppError::Other(
            "this project keeps its database in the folder".into(),
        ));
    }
    let guard = ExportGuard(base.join(SNAPSHOT));
    export(db, store, base, &guard.0)
}

/// Removes the snapshot on every exit, a panic included.
struct ExportGuard(std::path::PathBuf);

impl Drop for ExportGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn export(
    db: &Db,
    store: &dyn ProjectStore,
    base: &Path,
    snapshot: &Path,
) -> AppResult<ExportOutcome> {
    let previous = read_record(base);
    // Always asked: on SAF it also drops cached entries of a copy or folder that
    // was deleted outside the app, so the write below cannot trust them.
    let on_disk = store.stat(COPY_REL)?;
    let wal = wal_fingerprint(store.stat(WAL_REL)?);
    // A copy or a WAL this device neither wrote nor accepted is another
    // device's work. Checked before the snapshot, so a folder in conflict
    // costs nothing on every timer tick.
    let recorded = previous.as_ref().map(|p| (p.size, p.mtime));
    let recorded_wal = previous.as_ref().and_then(|p| p.wal);
    if on_disk.is_some_and(|f| recorded != Some(f)) || wal.is_some_and(|w| recorded_wal != Some(w))
    {
        return Ok(ExportOutcome::Conflict);
    }
    let _ = std::fs::remove_file(snapshot);
    let target = snapshot.to_string_lossy().replace('\'', "''");
    db.call(move |conn| {
        conn.execute_batch(&format!("VACUUM INTO '{target}'"))?;
        Ok(())
    })?;
    let bytes = std::fs::read(snapshot)?;
    let hash = xxhash_rust::xxh3::xxh3_64(&bytes);
    if let Some(prev) = &previous {
        // The size and mtime check catches a deleted copy.
        if prev.hash == hash && on_disk == recorded && wal.is_none() {
            return Ok(ExportOutcome::Unchanged);
        }
    }
    // The copy of a device with other file ids replaces this one. A cache of
    // such a device holds keys that would match other files.
    if previous.as_ref().is_some_and(|p| p.hash == 0) && store.exists(THUMBS_REL)? {
        store.remove_dir_all(THUMBS_REL)?;
    }
    store.create_dir_all(".cullant")?;
    // A desktop leaves these next to its database. Next to the new main file
    // they would be applied to it.
    for rel in [WAL_REL, ".cullant/cullant.db-shm"] {
        if store.exists(rel)? {
            store.remove_file(rel)?;
        }
    }
    store.write_replacing(COPY_REL, &bytes, "application/octet-stream")?;
    let (size, mtime) = store
        .stat(COPY_REL)?
        .ok_or_else(|| AppError::Other("the folder copy is missing after the write".into()))?;
    write_record(
        base,
        &Record {
            hash,
            size,
            mtime,
            wal: None,
        },
    )?;
    Ok(ExportOutcome::Written)
}

fn read_record(base: &Path) -> Option<Record> {
    let raw = std::fs::read(base.join(RECORD)).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn write_record(base: &Path, record: &Record) -> AppResult<()> {
    // A torn record would make this device's own copy look foreign.
    let temporary = base.join(format!("{RECORD}.tmp"));
    std::fs::write(&temporary, serde_json::to_vec(record).unwrap())?;
    std::fs::rename(&temporary, base.join(RECORD))?;
    Ok(())
}

/// True when this device wrote the folder copy it sees now.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn is_own(base: &Path, fp: Fingerprint) -> bool {
    read_record(base).is_some_and(|r| (r.size, r.mtime, r.wal) == (fp.size, fp.mtime, fp.wal))
}

/// The user kept this device's data. Remember the folder copy so it does not
/// ask again. The hash is 0, so the next export writes over the copy.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn accept(base: &Path, fp: Fingerprint) -> AppResult<()> {
    write_record(base, &Record::new(0, fp))
}

/// Replace the live database with a copy taken from the folder. The copy sits
/// in a private directory as `cullant.db`; a `cullant.db-wal` next to it is
/// applied. Nothing in `base` changes unless the copy is sound.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn import_files(base: &Path, copied_db: &Path, folder: Fingerprint) -> AppResult<()> {
    let hash = xxhash_rust::xxh3::xxh3_64(&std::fs::read(copied_db)?);
    prepare_copy(copied_db)?;
    let live = base.join(".cullant");
    std::fs::create_dir_all(&live)?;
    // The old companions must go first: next to the new main file they would
    // be applied to it. The rename then replaces the main file in one step, so
    // a failure or a kill never leaves the project without a database.
    for name in ["cullant.db-wal", "cullant.db-shm"] {
        ignore_missing(std::fs::remove_file(live.join(name)))?;
    }
    std::fs::rename(copied_db, live.join("cullant.db"))?;
    write_record(base, &Record::new(hash, folder))?;
    // The cache keys hold file ids of the old database. Last, because the
    // import is complete without it: a leftover cache only wastes space.
    if let Err(error) = ignore_missing(std::fs::remove_dir_all(live.join("thumbs"))) {
        tracing::warn!("could not remove the old thumbnail cache: {error}");
    }
    Ok(())
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn ignore_missing(result: std::io::Result<()>) -> AppResult<()> {
    match result {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Check the copy, then make it one plain file for this device.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn prepare_copy(path: &Path) -> AppResult<()> {
    let conn = rusqlite::Connection::open(path)?;
    let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(AppError::Other(
            "The data in this folder is damaged.".into(),
        ));
    }
    let version = crate::db::migrations::current_version(&conn)?;
    if version > crate::db::migrations::latest_version() {
        return Err(AppError::Other(
            "This folder data needs a newer Cullant.".into(),
        ));
    }
    // The cached ids belong to the device that wrote the copy.
    if version >= 2 {
        conn.execute("DELETE FROM saf_documents", [])?;
    }
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LocalFsStore;

    /// A store that claims to mirror, over a plain folder.
    struct Mirroring(LocalFsStore);

    macro_rules! forward {
        ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
            $(fn $name(&self, $($arg: $ty),*) -> $ret { self.0.$name($($arg),*) })*
        };
    }

    impl ProjectStore for Mirroring {
        fn list_recursive(
            &self,
            skip_dirs: &[&str],
            progress: &mut dyn FnMut(usize),
            cancelled: &dyn Fn() -> bool,
        ) -> AppResult<Vec<crate::store::StoreEntry>> {
            self.0.list_recursive(skip_dirs, progress, cancelled)
        }
        forward! {
            open_read(rel: &str) -> AppResult<std::fs::File>;
            open_write(rel: &str, mime: &str) -> AppResult<std::fs::File>;
            create_dir_all(rel: &str) -> AppResult<()>;
            rename_in_place(rel: &str, new_name: &str) -> AppResult<String>;
            move_to(rel: &str, parent: &str) -> AppResult<String>;
            move_file(from: &str, to: &str) -> AppResult<()>;
            write_replacing(rel: &str, bytes: &[u8], mime: &str) -> AppResult<()>;
            copy(from: &str, to: &str) -> AppResult<()>;
            remove_file(rel: &str) -> AppResult<()>;
            remove_dir_all(rel: &str) -> AppResult<()>;
            exists(rel: &str) -> AppResult<bool>;
            stat(rel: &str) -> AppResult<Option<(i64, i64)>>;
        }
        fn mirrors_metadata(&self) -> bool {
            true
        }
    }

    /// A mirroring store whose writes always fail.
    struct Failing(Mirroring);

    impl ProjectStore for Failing {
        fn move_file(&self, _from: &str, _to: &str) -> AppResult<()> {
            Err(AppError::Other("move failed".into()))
        }
        fn list_recursive(
            &self,
            skip_dirs: &[&str],
            progress: &mut dyn FnMut(usize),
            cancelled: &dyn Fn() -> bool,
        ) -> AppResult<Vec<crate::store::StoreEntry>> {
            self.0.list_recursive(skip_dirs, progress, cancelled)
        }
        forward! {
            open_read(rel: &str) -> AppResult<std::fs::File>;
            open_write(rel: &str, mime: &str) -> AppResult<std::fs::File>;
            create_dir_all(rel: &str) -> AppResult<()>;
            rename_in_place(rel: &str, new_name: &str) -> AppResult<String>;
            move_to(rel: &str, parent: &str) -> AppResult<String>;
            copy(from: &str, to: &str) -> AppResult<()>;
            remove_file(rel: &str) -> AppResult<()>;
            remove_dir_all(rel: &str) -> AppResult<()>;
            exists(rel: &str) -> AppResult<bool>;
            stat(rel: &str) -> AppResult<Option<(i64, i64)>>;
        }
        fn mirrors_metadata(&self) -> bool {
            true
        }
    }

    fn setup() -> (tempfile::TempDir, tempfile::TempDir, Db, Mirroring) {
        let base = tempfile::tempdir().unwrap();
        let tree = tempfile::tempdir().unwrap();
        let db = Db::open(base.path()).unwrap();
        db.call(|conn| {
            conn.execute("INSERT INTO groups (created_at) VALUES (0)", [])?;
            conn.execute(
                "INSERT INTO files (rel_path, basename, dir, ext, kind, size, mtime, group_id)
                 VALUES ('a.jpg', 'a', '', 'jpg', 1, 1, 1, 1)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let store = Mirroring(LocalFsStore::new(tree.path()));
        (base, tree, db, store)
    }

    fn run(base: &tempfile::TempDir, db: &Db, store: &Mirroring) -> ExportOutcome {
        export_folder_copy(db, store, base.path()).unwrap()
    }

    #[test]
    fn first_export_writes_a_readable_copy_then_nothing_changes() {
        let (base, tree, db, store) = setup();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Written);
        let copy = rusqlite::Connection::open(tree.path().join(COPY_REL)).unwrap();
        let rows: i64 = copy
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        assert!(!base.path().join(SNAPSHOT).exists());
        assert_eq!(run(&base, &db, &store), ExportOutcome::Unchanged);
    }

    #[test]
    fn a_changed_row_is_written_again() {
        let (base, tree, db, store) = setup();
        run(&base, &db, &store);
        db.call(|conn| {
            conn.execute("UPDATE files SET rating = 4", [])?;
            Ok(())
        })
        .unwrap();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Written);
        let copy = rusqlite::Connection::open(tree.path().join(COPY_REL)).unwrap();
        let rating: i64 = copy
            .query_row("SELECT rating FROM files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rating, 4);
    }

    #[test]
    fn the_record_matches_the_copy_on_disk() {
        let (base, _tree, db, store) = setup();
        run(&base, &db, &store);
        let record: Record =
            serde_json::from_slice(&std::fs::read(base.path().join(RECORD)).unwrap()).unwrap();
        assert_eq!(
            store.stat(COPY_REL).unwrap(),
            Some((record.size, record.mtime))
        );
    }

    #[test]
    fn a_deleted_copy_is_written_again_with_the_same_hash() {
        let (base, tree, db, store) = setup();
        run(&base, &db, &store);
        std::fs::remove_file(tree.path().join(COPY_REL)).unwrap();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Written);
        assert!(tree.path().join(COPY_REL).exists());
    }

    #[test]
    fn a_store_that_does_not_mirror_is_refused() {
        let (base, tree, db, _) = setup();
        let plain = LocalFsStore::new(tree.path());
        assert!(export_folder_copy(&db, &plain, base.path()).is_err());
    }

    #[test]
    fn a_failed_write_leaves_no_snapshot_no_staged_file() {
        let (base, tree, db, store) = setup();
        let failing = Failing(store);
        assert!(export_folder_copy(&db, &failing, base.path()).is_err());
        assert!(!base.path().join(SNAPSHOT).exists());
        let left = std::fs::read_dir(tree.path().join(".cullant"))
            .unwrap()
            .count();
        assert_eq!(left, 0, "a staged file was left behind");
    }

    /// A database file with `rows` files and one `saf_documents` row, in its own dir.
    fn copy_with_rows(rows: i64) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        db.call(move |conn| {
            conn.execute("INSERT INTO groups (created_at) VALUES (0)", [])?;
            for i in 0..rows {
                conn.execute(
                    "INSERT INTO files (rel_path, basename, dir, ext, kind, size, mtime, group_id)
                     VALUES (?1, 'a', '', 'jpg', 1, 1, 1, 1)",
                    [format!("{i}.jpg")],
                )?;
            }
            conn.execute(
                "INSERT INTO saf_documents (rel_path, document_id, is_dir) VALUES ('a', 'x', 0)",
                [],
            )?;
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
            Ok(())
        })
        .unwrap();
        drop(db);
        let path = dir.path().join("cullant.db");
        std::fs::copy(dir.path().join(".cullant").join("cullant.db"), &path).unwrap();
        (dir, path)
    }

    fn count(path: &Path, table: &str) -> i64 {
        rusqlite::Connection::open(path)
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn live(base: &tempfile::TempDir) -> std::path::PathBuf {
        base.path().join(".cullant").join("cullant.db")
    }

    fn fp(size: i64, mtime: i64) -> Fingerprint {
        Fingerprint {
            size,
            mtime,
            wal: None,
        }
    }

    /// The fingerprint of the folder copy and its WAL, as the store sees them.
    fn on_disk(store: &Mirroring) -> Fingerprint {
        let (size, mtime) = store.stat(COPY_REL).unwrap().unwrap();
        Fingerprint {
            size,
            mtime,
            wal: wal_fingerprint(store.stat(WAL_REL).unwrap()),
        }
    }

    #[test]
    fn a_copy_is_own_only_with_the_recorded_size_and_mtime() {
        let (base, _tree, db, store) = setup();
        assert!(!is_own(base.path(), fp(1, 1)));
        run(&base, &db, &store);
        let r = read_record(base.path()).unwrap();
        assert!(is_own(base.path(), fp(r.size, r.mtime)));
        assert!(!is_own(base.path(), fp(r.size + 1, r.mtime)));
        assert!(!is_own(base.path(), fp(r.size, r.mtime + 1)));
        let with_wal = Fingerprint {
            wal: Some((8, 1)),
            ..fp(r.size, r.mtime)
        };
        assert!(!is_own(base.path(), with_wal));
    }

    #[test]
    fn a_foreign_copy_is_never_overwritten_until_accepted() {
        let (base, tree, db, store) = setup();
        std::fs::create_dir_all(tree.path().join(".cullant")).unwrap();
        let copy = tree.path().join(COPY_REL);
        std::fs::write(&copy, b"foreign").unwrap();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Conflict);
        assert_eq!(std::fs::read(&copy).unwrap(), b"foreign");
        assert!(!base.path().join(RECORD).exists());
        accept(base.path(), on_disk(&store)).unwrap();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Written);
    }

    #[test]
    fn a_copy_changed_after_our_write_is_a_conflict() {
        let (base, tree, db, store) = setup();
        run(&base, &db, &store);
        std::fs::write(tree.path().join(COPY_REL), b"other device").unwrap();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Conflict);
    }

    #[test]
    fn an_accepted_copy_is_own_and_the_next_export_writes() {
        let (base, _tree, db, store) = setup();
        run(&base, &db, &store);
        let r = read_record(base.path()).unwrap();
        std::fs::remove_file(base.path().join(RECORD)).unwrap();
        accept(base.path(), fp(r.size, r.mtime)).unwrap();
        assert!(is_own(base.path(), fp(r.size, r.mtime)));
        assert_eq!(run(&base, &db, &store), ExportOutcome::Written);
    }

    #[test]
    fn an_accepted_copy_is_replaced_without_the_cache_of_its_device() {
        let (base, tree, db, store) = setup();
        run(&base, &db, &store);
        let r = read_record(base.path()).unwrap();
        accept(base.path(), fp(r.size, r.mtime)).unwrap();
        std::fs::create_dir_all(tree.path().join(THUMBS_REL)).unwrap();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Written);
        assert!(!tree.path().join(THUMBS_REL).exists());
        std::fs::create_dir_all(tree.path().join(THUMBS_REL)).unwrap();
        db.call(|c| Ok(c.execute("UPDATE files SET size = 2", [])?))
            .unwrap();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Written);
        assert!(tree.path().join(THUMBS_REL).exists(), "own cache is kept");
    }

    #[test]
    fn an_import_replaces_the_live_database_and_resets_caches() {
        let (base, _tree, db, _store) = setup();
        drop(db);
        let thumbs = base.path().join(".cullant").join("thumbs");
        std::fs::create_dir_all(&thumbs).unwrap();
        std::fs::write(thumbs.join("t.jpg"), b"x").unwrap();
        let (_dir, copy) = copy_with_rows(3);
        import_files(base.path(), &copy, fp(77, 88)).unwrap();
        assert_eq!(count(&live(&base), "files"), 3);
        assert_eq!(count(&live(&base), "saf_documents"), 0);
        assert!(!thumbs.exists());
        assert!(!copy.exists());
        assert!(is_own(base.path(), fp(77, 88)));
    }

    #[test]
    fn rows_that_exist_only_in_the_wal_are_imported() {
        let (base, _tree, db, _store) = setup();
        drop(db);
        let src = tempfile::tempdir().unwrap();
        let (_dir, seed) = copy_with_rows(0);
        let origin = src.path().join("cullant.db");
        std::fs::copy(seed, &origin).unwrap();
        let conn = rusqlite::Connection::open(&origin).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        conn.execute(
            "INSERT INTO files (rel_path, basename, dir, ext, kind, size, mtime, group_id)
             VALUES ('w.jpg', 'w', '', 'jpg', 1, 1, 1, 1)",
            [],
        )
        .unwrap();
        let staged = tempfile::tempdir().unwrap();
        let copy = staged.path().join("cullant.db");
        std::fs::copy(&origin, &copy).unwrap();
        std::fs::copy(
            src.path().join("cullant.db-wal"),
            staged.path().join("cullant.db-wal"),
        )
        .unwrap();
        drop(conn);
        import_files(base.path(), &copy, fp(1, 1)).unwrap();
        assert_eq!(count(&live(&base), "files"), 1);
    }

    #[test]
    fn a_newer_schema_is_refused_and_the_live_database_stays() {
        let (base, _tree, db, _store) = setup();
        drop(db);
        let (_dir, copy) = copy_with_rows(2);
        let newer = crate::db::migrations::latest_version() + 1;
        rusqlite::Connection::open(&copy)
            .unwrap()
            .execute("UPDATE schema_meta SET version = ?1", [newer])
            .unwrap();
        let error = import_files(base.path(), &copy, fp(1, 1)).unwrap_err();
        assert!(error.to_string().contains("newer Cullant"));
        assert_eq!(count(&live(&base), "files"), 1);
    }

    #[test]
    fn an_export_removes_an_empty_wal_and_the_shm_of_a_desktop() {
        let (base, tree, db, store) = setup();
        let dir = tree.path().join(".cullant");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cullant.db-wal"), b"").unwrap();
        std::fs::write(dir.join("cullant.db-shm"), b"stale").unwrap();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Written);
        assert!(!dir.join("cullant.db-wal").exists());
        assert!(!dir.join("cullant.db-shm").exists());
        assert_eq!(count(&dir.join("cullant.db"), "files"), 1);
    }

    #[test]
    fn a_foreign_wal_next_to_our_copy_is_a_conflict_and_nothing_changes() {
        let (base, tree, db, store) = setup();
        run(&base, &db, &store);
        let copy = tree.path().join(COPY_REL);
        let main = std::fs::read(&copy).unwrap();
        let wal = tree.path().join(WAL_REL);
        std::fs::write(&wal, b"desktop edits").unwrap();
        let mine = read_record(base.path()).unwrap();
        assert!(!is_own(base.path(), on_disk(&store)));
        assert!(is_own(base.path(), fp(mine.size, mine.mtime)));
        db.call(|conn| {
            conn.execute("UPDATE files SET rating = 4", [])?;
            Ok(())
        })
        .unwrap();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Conflict);
        assert_eq!(std::fs::read(&wal).unwrap(), b"desktop edits");
        assert_eq!(std::fs::read(&copy).unwrap(), main);
    }

    /// A folder copy with the WAL of a desktop that has the folder open.
    fn folder_copy_with_wal(tree: &tempfile::TempDir) {
        let dir = tree.path().join(".cullant");
        std::fs::create_dir_all(&dir).unwrap();
        let (_seed_dir, seed) = copy_with_rows(0);
        std::fs::copy(seed, dir.join("cullant.db")).unwrap();
        let conn = rusqlite::Connection::open(dir.join("cullant.db")).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        conn.execute(
            "INSERT INTO files (rel_path, basename, dir, ext, kind, size, mtime, group_id)
             VALUES ('w.jpg', 'w', '', 'jpg', 1, 1, 1, 1)",
            [],
        )
        .unwrap();
        // The close checkpoints the WAL, so keep the files as they are now.
        let held = tempfile::tempdir().unwrap();
        for name in ["cullant.db", "cullant.db-wal"] {
            std::fs::copy(dir.join(name), held.path().join(name)).unwrap();
        }
        drop(conn);
        for name in ["cullant.db", "cullant.db-wal"] {
            std::fs::copy(held.path().join(name), dir.join(name)).unwrap();
        }
        let _ = std::fs::remove_file(dir.join("cullant.db-shm"));
    }

    /// Import the folder copy as the SAF import does: stage the files, then
    /// import with the fingerprint of the folder files.
    fn import_from(base: &tempfile::TempDir, tree: &tempfile::TempDir, store: &Mirroring) {
        let staged = tempfile::tempdir().unwrap();
        for name in ["cullant.db", "cullant.db-wal"] {
            let from = tree.path().join(".cullant").join(name);
            if from.exists() {
                std::fs::copy(from, staged.path().join(name)).unwrap();
            }
        }
        import_files(
            base.path(),
            &staged.path().join("cullant.db"),
            on_disk(store),
        )
        .unwrap();
    }

    #[test]
    fn an_imported_copy_is_own_and_the_next_export_writes() {
        let (base, tree, db, store) = setup();
        drop(db);
        let (_dir, seed) = copy_with_rows(2);
        std::fs::create_dir_all(tree.path().join(".cullant")).unwrap();
        std::fs::copy(seed, tree.path().join(COPY_REL)).unwrap();
        import_from(&base, &tree, &store);
        assert!(is_own(base.path(), on_disk(&store)));
        let db = Db::open(base.path()).unwrap();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Written);
    }

    #[test]
    fn an_imported_wal_is_removed_by_the_next_export() {
        let (base, tree, db, store) = setup();
        drop(db);
        folder_copy_with_wal(&tree);
        assert!(on_disk(&store).wal.is_some());
        import_from(&base, &tree, &store);
        assert!(is_own(base.path(), on_disk(&store)));
        let db = Db::open(base.path()).unwrap();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Written);
        assert!(!tree.path().join(WAL_REL).exists());
        assert_eq!(count(&tree.path().join(COPY_REL), "files"), 1);
    }

    #[test]
    fn an_accepted_wal_is_removed_by_the_next_export() {
        let (base, tree, db, store) = setup();
        folder_copy_with_wal(&tree);
        assert_eq!(run(&base, &db, &store), ExportOutcome::Conflict);
        accept(base.path(), on_disk(&store)).unwrap();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Written);
        assert!(!tree.path().join(WAL_REL).exists());
    }

    #[test]
    fn a_record_without_a_wal_field_is_read() {
        let base = tempfile::tempdir().unwrap();
        let old = br#"{"hash":1,"size":2,"mtime":3}"#;
        std::fs::write(base.path().join(RECORD), old).unwrap();
        assert!(is_own(base.path(), fp(2, 3)));
    }

    #[test]
    fn a_thumbnail_cache_that_cannot_be_removed_does_not_fail_the_import() {
        let (base, _tree, db, _store) = setup();
        drop(db);
        // A file, not a directory: remove_dir_all fails on it.
        std::fs::write(base.path().join(".cullant").join("thumbs"), b"x").unwrap();
        let (_dir, copy) = copy_with_rows(3);
        import_files(base.path(), &copy, fp(1, 1)).unwrap();
        assert_eq!(count(&live(&base), "files"), 3);
    }

    #[test]
    fn a_damaged_file_is_refused_and_the_live_database_stays() {
        let (base, _tree, db, _store) = setup();
        drop(db);
        let junk = tempfile::tempdir().unwrap();
        let copy = junk.path().join("cullant.db");
        std::fs::write(&copy, vec![7u8; 4096]).unwrap();
        assert!(import_files(base.path(), &copy, fp(1, 1)).is_err());
        assert_eq!(count(&live(&base), "files"), 1);
    }
}
