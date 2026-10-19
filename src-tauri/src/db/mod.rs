pub mod migrations;
pub mod sql;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// How many `Db` handles are alive per database file. A handle can outlive its
/// project: a scan or a command keeps its `Arc` after the project is closed.
static OPEN: Mutex<BTreeMap<PathBuf, usize>> = Mutex::new(BTreeMap::new());

/// How long a reader/checkpoint waits on a briefly-held lock before erroring.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Attempts to open the database file, and the pause between them. A project
/// lives on whatever the photos live on — an external disk that has spun down,
/// a card reader — so the first open can fail on storage that is about to
/// answer perfectly well a moment later.
const OPEN_ATTEMPTS: u32 = 4;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(400);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Handle to the project database. Writes go through a single writer thread
/// that owns one connection (they must serialize anyway); reads can instead
/// borrow a connection from a small read-only pool and run concurrently on the
/// calling thread — WAL lets many readers proceed alongside the writer. SQLite
/// in WAL mode makes this pattern both simple and fast.
pub struct Db {
    tx: mpsc::Sender<Job>,
    readers: ReaderPool,
    path: PathBuf,
    generation: u64,
    writer: Option<thread::JoinHandle<()>>,
}

/// Waits until every connection is closed, so a caller may replace the files
/// right after the last handle goes. Without it the writer closes late and its
/// checkpoint can unlink a `-wal` that already belongs to a newer database.
impl Drop for Db {
    fn drop(&mut self) {
        let Some(writer) = self.writer.take() else {
            return;
        };
        // Ends the writer loop. The writer thread never owns a `Db`: its jobs
        // get only the connection, so this join cannot wait on itself.
        self.tx = mpsc::channel().0;
        let _ = writer.join();
        self.readers.conns.lock().unwrap().clear();
        let mut open = OPEN.lock().unwrap();
        if let Some(count) = open.get_mut(&self.path) {
            *count -= 1;
            if *count == 0 {
                open.remove(&self.path);
            }
        }
    }
}

/// A bounded set of read-only connections handed out to whatever thread needs a
/// read. Connections are opened read-write but pinned to `query_only = ON`: a
/// purely `SQLITE_OPEN_READ_ONLY` connection can't touch the `-shm` index of a
/// live WAL database, whereas `query_only` still forbids writes while allowing
/// the WAL reads to work.
struct ReaderPool {
    conns: Mutex<Vec<Connection>>,
    available: Condvar,
}

impl ReaderPool {
    /// Borrow a connection, waiting if all are currently in use. The returned
    /// guard checks it back in on drop (even on panic).
    fn checkout(&self) -> ReaderGuard<'_> {
        let mut conns = self.conns.lock().unwrap();
        loop {
            if let Some(conn) = conns.pop() {
                return ReaderGuard {
                    pool: self,
                    conn: Some(conn),
                };
            }
            conns = self.available.wait(conns).unwrap();
        }
    }

    fn checkin(&self, conn: Connection) {
        self.conns.lock().unwrap().push(conn);
        self.available.notify_one();
    }
}

struct ReaderGuard<'a> {
    pool: &'a ReaderPool,
    conn: Option<Connection>,
}

impl Drop for ReaderGuard<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.checkin(conn);
        }
    }
}

impl Db {
    /// Open (creating if needed) `<project_root>/.cullant/cullant.db` and run
    /// pending migrations.
    ///
    /// Retries a failed open before giving up. `SQLITE_CANTOPEN` off removable
    /// media usually means the drive was still waking, not that the project is
    /// broken — and reporting "unable to open database file" for a project that
    /// opens on the next try is the wrong answer.
    pub fn open(project_root: &Path) -> AppResult<Db> {
        let mut last_err = None;
        for attempt in 1..=OPEN_ATTEMPTS {
            match Self::open_once(project_root) {
                Ok(db) => {
                    if attempt > 1 {
                        tracing::info!("opened project database on attempt {attempt}");
                    }
                    return Ok(db);
                }
                Err(e) => {
                    tracing::warn!("opening project database failed (attempt {attempt}): {e}");
                    last_err = Some(e);
                    if attempt < OPEN_ATTEMPTS {
                        thread::sleep(OPEN_RETRY_DELAY);
                    }
                }
            }
        }
        Err(last_err.expect("at least one attempt runs"))
    }

    fn open_once(project_root: &Path) -> AppResult<Db> {
        let dir = project_root.join(".cullant");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("cullant.db");

        let mut conn = Connection::open(&path)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrations::run(&mut conn)?;
        crate::engine::tags::seed_defaults(&conn)?;

        // Reader pool. Opened after the writer set WAL (a persistent property of
        // the file), so these connections see a WAL database. Sized modestly:
        // reads are short, and each connection carries its own page cache.
        let reader_count = std::thread::available_parallelism()
            .map(|n| n.get().clamp(2, 4))
            .unwrap_or(2);
        let mut reader_conns = Vec::with_capacity(reader_count);
        for _ in 0..reader_count {
            let rconn = Connection::open(&path)?;
            rconn.busy_timeout(BUSY_TIMEOUT)?;
            rconn.pragma_update(None, "query_only", "ON")?;
            reader_conns.push(rconn);
        }
        let readers = ReaderPool {
            conns: Mutex::new(reader_conns),
            available: Condvar::new(),
        };

        let (tx, rx) = mpsc::channel::<Job>();
        let writer = thread::Builder::new()
            .name("db-writer".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    // One panicking job must not kill the only writer: every
                    // later write would fail with "database thread is gone"
                    // until the project is reopened. The panicking caller gets
                    // "dropped the job"; an open transaction rolls back on unwind.
                    if let Err(payload) =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(&mut conn)))
                    {
                        tracing::error!(
                            "database job panicked: {}",
                            crate::panic_message(payload.as_ref())
                        );
                    }
                }
            })?;

        *OPEN.lock().unwrap().entry(path.clone()).or_insert(0) += 1;
        Ok(Db {
            tx,
            readers,
            path,
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            writer: Some(writer),
        })
    }

    /// Run a closure on the single writer thread and wait for its result. Use
    /// for any statement that writes, or a read that must observe writes issued
    /// earlier in the same logical step from this same path.
    pub fn call<T, F>(&self, f: F) -> AppResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> AppResult<T> + Send + 'static,
    {
        let _profile = crate::photo_profile::span("db.write_wait");
        let (result_tx, result_rx) = mpsc::channel();
        self.tx
            .send(Box::new(move |conn| {
                let _profile = crate::photo_profile::span("db.write_execute");
                let _ = result_tx.send(f(conn));
            }))
            .map_err(|_| AppError::Other("database thread is gone".into()))?;
        result_rx
            .recv()
            .map_err(|_| AppError::Other("database thread dropped the job".into()))?
    }

    /// Run a read-only closure on a pooled connection, on the CALLING thread —
    /// no writer-thread round-trip, and concurrent with other reads and the
    /// writer. Only for pure `SELECT`s that tolerate seeing the last committed
    /// snapshot (the WAL default); a read routed here after an awaited [`call`]
    /// write still sees that write, since `call` returns only once committed.
    /// The closure runs on the caller's stack, so it need not be `Send`/`'static`.
    pub fn call_read<T, F>(&self, f: F) -> AppResult<T>
    where
        F: FnOnce(&Connection) -> AppResult<T>,
    {
        let _profile = crate::photo_profile::span("db.read");
        let guard = self.readers.checkout();
        // `conn` is always `Some` for a freshly checked-out guard.
        f(guard
            .conn
            .as_ref()
            .expect("reader guard holds a connection"))
    }

    /// True while any handle to the database file at `path` is alive.
    pub fn is_open_at(path: &Path) -> bool {
        OPEN.lock().unwrap().contains_key(path)
    }

    /// Wait until no handle to `path` is alive. False when `timeout` passes first.
    pub fn wait_closed_at(path: &Path, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while Self::is_open_at(path) {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(50));
        }
        true
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn project_root(&self) -> String {
        self.call_read(|conn| {
            Ok(
                conn.query_row("SELECT root_path FROM project WHERE id = 1", [], |row| {
                    row.get::<_, String>(0)
                })?,
            )
        })
        .unwrap_or_else(|_| {
            self.path
                .parent()
                .and_then(Path::parent)
                .unwrap_or(Path::new(""))
                .to_string_lossy()
                .into_owned()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_event_identity_uses_the_stored_saf_uri() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        db.call(|conn| {
            conn.execute(
                "INSERT INTO project (id, root_path, created_at) VALUES (1, 'content://provider/tree/photos', 0)",
                [],
            )?;
            Ok(())
        }).unwrap();
        assert_eq!(db.project_root(), "content://provider/tree/photos");
    }

    #[test]
    fn reopened_projects_have_a_distinct_generation() {
        let dir = tempfile::tempdir().unwrap();
        let first = Db::open(dir.path()).unwrap();
        let second = Db::open(dir.path()).unwrap();
        assert_ne!(first.generation(), second.generation());
    }

    #[test]
    fn a_database_counts_as_open_until_its_last_handle_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let first = std::sync::Arc::new(Db::open(dir.path()).unwrap());
        let second = first.clone();
        let path = first.path().to_path_buf();
        assert!(Db::is_open_at(&path));
        drop(first);
        assert!(!Db::wait_closed_at(&path, Duration::from_millis(120)));
        drop(second);
        assert!(Db::wait_closed_at(&path, Duration::from_millis(120)));
    }

    #[test]
    fn open_creates_db_and_applies_schema() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();

        assert!(db.path().exists());
        let version = db.call(|conn| migrations::current_version(conn)).unwrap();
        assert_eq!(version, 12);

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
        assert_eq!(version, 12);
    }

    #[test]
    fn read_pool_sees_committed_writes_and_forbids_writes() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();

        // A write via the writer thread, then a read via the pool must observe
        // it (call returns only once the write has committed to the WAL).
        db.call(|conn| {
            conn.execute("INSERT INTO settings (key, value) VALUES ('k', 'v1')", [])?;
            Ok(())
        })
        .unwrap();
        let v: String = db
            .call_read(|conn| {
                Ok(
                    conn.query_row("SELECT value FROM settings WHERE key = 'k'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(v, "v1");

        // The pool is query_only: a write attempt through it must fail rather
        // than mutate the database.
        let write_via_reader = db.call_read(|conn| {
            conn.execute("UPDATE settings SET value = 'v2' WHERE key = 'k'", [])?;
            Ok(())
        });
        assert!(
            write_via_reader.is_err(),
            "query_only reader must reject writes"
        );
        let still: String = db
            .call_read(|conn| {
                Ok(
                    conn.query_row("SELECT value FROM settings WHERE key = 'k'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(still, "v1");
    }

    #[test]
    fn read_pool_serves_concurrent_readers() {
        use std::sync::Arc;
        use std::thread;

        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open(dir.path()).unwrap());
        db.call(|conn| {
            conn.execute("INSERT INTO settings (key, value) VALUES ('k', 'v')", [])?;
            Ok(())
        })
        .unwrap();

        // More concurrent readers than pooled connections: extras block on
        // checkout and are served as connections free up (no deadlock, no error).
        let mut handles = Vec::new();
        for _ in 0..16 {
            let db = db.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..50 {
                    let v: String = db
                        .call_read(|conn| {
                            Ok(conn.query_row(
                                "SELECT value FROM settings WHERE key = 'k'",
                                [],
                                |r| r.get(0),
                            )?)
                        })
                        .unwrap();
                    assert_eq!(v, "v");
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }
}
