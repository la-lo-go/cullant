pub mod migrations;

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// Handle to the project database. All access goes through a single writer
/// thread that owns the connection; callers submit closures and wait for the
/// result. SQLite in WAL mode makes this pattern both simple and fast.
pub struct Db {
    tx: mpsc::Sender<Job>,
    path: PathBuf,
}

impl Db {
    /// Open (creating if needed) `<project_root>/.cullant/cullant.db` and run
    /// pending migrations.
    pub fn open(project_root: &Path) -> AppResult<Db> {
        let dir = project_root.join(".cullant");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("cullant.db");

        let mut conn = Connection::open(&path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrations::run(&mut conn)?;
        crate::engine::tags::seed_defaults(&conn)?;

        let (tx, rx) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("db-writer".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job(&mut conn);
                }
            })?;

        Ok(Db { tx, path })
    }

    /// Run a closure on the database thread and wait for its result.
    pub fn call<T, F>(&self, f: F) -> AppResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> AppResult<T> + Send + 'static,
    {
        let (result_tx, result_rx) = mpsc::channel();
        self.tx
            .send(Box::new(move |conn| {
                let _ = result_tx.send(f(conn));
            }))
            .map_err(|_| AppError::Other("database thread is gone".into()))?;
        result_rx
            .recv()
            .map_err(|_| AppError::Other("database thread dropped the job".into()))?
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_db_and_applies_schema() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();

        assert!(db.path().exists());
        let version = db.call(|conn| migrations::current_version(conn)).unwrap();
        assert_eq!(version, 1);

        // All core tables exist and are queryable.
        let table_count = db
            .call(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'
                     AND name IN ('project','files','groups','task_tags','file_tags',
                                  'pending_actions','commits','commit_entries',
                                  'thumbnails','settings','file_analysis','similarity_clusters')",
                    [],
                    |row| row.get::<_, i64>(0),
                )?)
            })
            .unwrap();
        assert_eq!(table_count, 12);
    }

    #[test]
    fn reopen_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        drop(Db::open(dir.path()).unwrap());
        let db = Db::open(dir.path()).unwrap();
        let version = db.call(|conn| migrations::current_version(conn)).unwrap();
        assert_eq!(version, 1);
    }
}
