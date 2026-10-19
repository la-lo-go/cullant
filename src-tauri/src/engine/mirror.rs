//! Copy of the live project database into the project folder, for stores that
//! cannot keep the live database there (Android SAF).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::store::ProjectStore;

const COPY_REL: &str = ".cullant/cullant.db";
const SNAPSHOT: &str = "mirror.tmp.db";
const RECORD: &str = "mirror.json";

static EXPORTING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportOutcome {
    Written,
    Unchanged,
    Skipped,
}

/// What this device last wrote to the folder.
#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Record {
    hash: u64,
    size: i64,
    mtime: i64,
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
    if EXPORTING.swap(true, Ordering::Acquire) {
        return Ok(ExportOutcome::Skipped);
    }
    let snapshot = base.join(SNAPSHOT);
    let result = export(db, store, base, &snapshot);
    let _ = std::fs::remove_file(&snapshot);
    EXPORTING.store(false, Ordering::Release);
    result
}

fn export(
    db: &Db,
    store: &dyn ProjectStore,
    base: &Path,
    snapshot: &Path,
) -> AppResult<ExportOutcome> {
    let _ = std::fs::remove_file(snapshot);
    let target = snapshot.to_string_lossy().replace('\'', "''");
    db.call(move |conn| {
        conn.execute_batch(&format!("VACUUM INTO '{target}'"))?;
        Ok(())
    })?;
    let bytes = std::fs::read(snapshot)?;
    let hash = xxhash_rust::xxh3::xxh3_64(&bytes);
    let record_path = base.join(RECORD);
    let previous: Option<Record> = std::fs::read(&record_path)
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok());
    if let Some(prev) = previous {
        // The size and mtime check catches a copy that was deleted or replaced.
        if prev.hash == hash && store.stat(COPY_REL)? == Some((prev.size, prev.mtime)) {
            return Ok(ExportOutcome::Unchanged);
        }
    }
    store.create_dir_all(".cullant")?;
    store.write_replacing(COPY_REL, &bytes, "application/octet-stream")?;
    let (size, mtime) = store
        .stat(COPY_REL)?
        .ok_or_else(|| AppError::Other("the folder copy is missing after the write".into()))?;
    let record = Record { hash, size, mtime };
    std::fs::write(&record_path, serde_json::to_vec(&record).unwrap())?;
    Ok(ExportOutcome::Written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LocalFsStore;
    use std::sync::Mutex;

    // The exporting flag is process-wide, so the tests take turns.
    static SERIAL: Mutex<()> = Mutex::new(());

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
        let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
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
        let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
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
        let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
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
        let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (base, tree, db, store) = setup();
        run(&base, &db, &store);
        std::fs::remove_file(tree.path().join(COPY_REL)).unwrap();
        assert_eq!(run(&base, &db, &store), ExportOutcome::Written);
        assert!(tree.path().join(COPY_REL).exists());
    }

    #[test]
    fn a_store_that_does_not_mirror_is_refused() {
        let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (base, tree, db, _) = setup();
        let plain = LocalFsStore::new(tree.path());
        assert!(export_folder_copy(&db, &plain, base.path()).is_err());
    }

    #[test]
    fn a_second_export_during_one_is_skipped() {
        let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (base, _tree, db, store) = setup();
        EXPORTING.store(true, Ordering::SeqCst);
        let outcome = run(&base, &db, &store);
        EXPORTING.store(false, Ordering::SeqCst);
        assert_eq!(outcome, ExportOutcome::Skipped);
    }
}
