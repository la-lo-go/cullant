//! Real-filesystem [`ProjectStore`] — desktop projects and the private
//! `.cullant` sidecar on every platform. A thin wrapper over `std::fs` /
//! `walkdir` that preserves the exact behavior the app had before the storage
//! abstraction existed.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use walkdir::WalkDir;

use super::{split_parent, validate_relative, ProjectStore, StoreEntry, WALK_PROGRESS_EVERY};
use crate::error::{AppError, AppResult};

pub struct LocalFsStore {
    root: PathBuf,
}

impl LocalFsStore {
    pub fn new(root: impl Into<PathBuf>) -> LocalFsStore {
        let root = root.into();
        LocalFsStore {
            root: root.canonicalize().unwrap_or(root),
        }
    }

    fn abs(&self, rel: &str) -> AppResult<PathBuf> {
        validate_relative(rel)?;
        let path = self.root.join(rel);
        let mut ancestor = path.as_path();
        loop {
            match ancestor.canonicalize() {
                Ok(resolved) => {
                    if !resolved.starts_with(&self.root) {
                        return Err(AppError::Other(format!("path leaves the project: {rel}")));
                    }
                    return Ok(path);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    ancestor = ancestor.parent().ok_or(e)?;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

fn unix_secs(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl ProjectStore for LocalFsStore {
    fn folder_project_path(&self, rel: &str) -> AppResult<String> {
        super::validate_relative(rel)?;
        let path = self.root.join(rel);
        if !path.is_dir() {
            return Err(AppError::Other(format!("folder not found: {rel}")));
        }
        let path = path.canonicalize()?;
        if !path.starts_with(self.root.canonicalize()?) {
            return Err(AppError::Other(format!("path leaves the project: {rel}")));
        }
        Ok(path.to_string_lossy().into_owned())
    }

    fn list_recursive(
        &self,
        skip_dirs: &[&str],
        progress: &mut dyn FnMut(usize),
    ) -> AppResult<Vec<StoreEntry>> {
        let mut out = Vec::new();
        let walker = WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                if e.depth() == 0 {
                    return true; // never filter the project root itself
                }
                let name = e.file_name().to_string_lossy();
                !(e.file_type().is_dir()
                    && (skip_dirs.contains(&name.as_ref()) || name.starts_with('.')))
            });

        for entry in walker.flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&self.root) else {
                continue;
            };
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            out.push(StoreEntry {
                rel_path: rel.to_string_lossy().replace('\\', "/"),
                size: meta.len() as i64,
                mtime: meta.modified().map(unix_secs).unwrap_or(0),
            });
            if out.len() % WALK_PROGRESS_EVERY == 0 {
                progress(out.len());
            }
        }
        Ok(out)
    }

    fn open_read(&self, rel: &str) -> AppResult<std::fs::File> {
        Ok(std::fs::File::open(self.abs(rel)?)?)
    }

    fn open_write(&self, rel: &str, _mime_type: &str) -> AppResult<std::fs::File> {
        Ok(std::fs::File::create(self.abs(rel)?)?)
    }

    fn create_dir_all(&self, rel: &str) -> AppResult<()> {
        std::fs::create_dir_all(self.abs(rel)?)?;
        Ok(())
    }

    fn rename_in_place(&self, rel: &str, new_name: &str) -> AppResult<String> {
        if new_name.contains('/') || new_name.is_empty() {
            return Err(AppError::Other("invalid file name".into()));
        }
        let (parent, _) = split_parent(rel);
        let new_rel = if parent.is_empty() {
            new_name.to_string()
        } else {
            format!("{parent}/{new_name}")
        };
        self.move_file(rel, &new_rel)?;
        Ok(new_rel)
    }

    fn move_to(&self, rel: &str, new_parent_rel: &str) -> AppResult<String> {
        let (_, name) = split_parent(rel);
        let new_rel = if new_parent_rel.is_empty() {
            name.to_string()
        } else {
            format!("{new_parent_rel}/{name}")
        };
        self.move_file(rel, &new_rel)?;
        Ok(new_rel)
    }

    fn move_file(&self, from: &str, to: &str) -> AppResult<()> {
        let source = self.abs(from)?;
        let destination = self.abs(to)?;
        if destination.try_exists()? {
            return Err(AppError::Other(format!("target exists: {to}")));
        }
        std::fs::rename(source, destination)?;
        Ok(())
    }

    fn write_sidecar(&self, rel: &str, bytes: &[u8]) -> AppResult<()> {
        use std::io::Write;
        let destination = self.abs(rel)?;
        let temporary = super::temporary_sibling(self, rel)?;
        let staged = self.abs(&temporary)?;
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staged)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&staged, &destination)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&staged);
        }
        result
    }

    fn copy(&self, from_rel: &str, to_rel: &str) -> AppResult<()> {
        let mut source = std::fs::File::open(self.abs(from_rel)?)?;
        let destination = self.abs(to_rel)?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        if let Err(error) = std::io::copy(&mut source, &mut output).and_then(|_| output.sync_all())
        {
            drop(output);
            let _ = std::fs::remove_file(&destination);
            return Err(error.into());
        }
        Ok(())
    }

    fn remove_file(&self, rel: &str) -> AppResult<()> {
        std::fs::remove_file(self.abs(rel)?)?;
        Ok(())
    }

    fn exists(&self, rel: &str) -> AppResult<bool> {
        Ok(self.abs(rel)?.try_exists()?)
    }

    fn local_path(&self, rel: &str) -> Option<PathBuf> {
        self.abs(rel).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_regression_store_rejects_paths_outside_the_project_and_replacements() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("a.jpg"), b"source").unwrap();
        std::fs::write(root.join("b.jpg"), b"foreign").unwrap();
        std::fs::write(dir.path().join("outside.jpg"), b"outside").unwrap();
        let store = LocalFsStore::new(&root);
        for path in [
            "../outside.jpg",
            "..\\outside.jpg",
            "/outside.jpg",
            "C:/outside.jpg",
        ] {
            assert!(store.remove_file(path).is_err(), "accepted {path}");
        }
        assert!(store.copy("a.jpg", "b.jpg").is_err());
        assert!(store.rename_in_place("a.jpg", "b.jpg").is_err());
        assert_eq!(std::fs::read(root.join("b.jpg")).unwrap(), b"foreign");
        assert_eq!(
            std::fs::read(dir.path().join("outside.jpg")).unwrap(),
            b"outside"
        );
    }

    #[test]
    fn lists_files_skipping_marked_and_dot_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("shoot")).unwrap();
        std::fs::create_dir_all(root.join("_trash")).unwrap();
        std::fs::create_dir_all(root.join(".cullant")).unwrap();
        std::fs::write(root.join("shoot/a.jpg"), b"x").unwrap();
        std::fs::write(root.join("_trash/b.jpg"), b"x").unwrap();
        std::fs::write(root.join(".cullant/c.jpg"), b"x").unwrap();
        std::fs::write(root.join("top.jpg"), b"x").unwrap();

        let store = LocalFsStore::new(root);
        let mut rels: Vec<String> = store
            .list_recursive(&["_trash"], &mut |_| {})
            .unwrap()
            .into_iter()
            .map(|e| e.rel_path)
            .collect();
        rels.sort();
        assert_eq!(rels, vec!["shoot/a.jpg".to_string(), "top.jpg".to_string()]);
    }

    #[test]
    fn move_and_rename_compute_expected_rel_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let store = LocalFsStore::new(root);
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        std::fs::write(root.join("a/x.jpg"), b"x").unwrap();

        let moved = store.move_to("a/x.jpg", "b").unwrap();
        assert_eq!(moved, "b/x.jpg");
        assert!(root.join("b/x.jpg").exists());

        let renamed = store.rename_in_place("b/x.jpg", "y.jpg").unwrap();
        assert_eq!(renamed, "b/y.jpg");
        assert!(root.join("b/y.jpg").exists());
    }

    #[test]
    fn copy_and_delete_and_exists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let store = LocalFsStore::new(root);
        std::fs::write(root.join("x.jpg"), b"hello").unwrap();

        store.copy("x.jpg", "y.jpg").unwrap();
        assert!(store.exists("y.jpg").unwrap());
        store.remove_file("y.jpg").unwrap();
        assert!(!store.exists("y.jpg").unwrap());
        assert!(store.exists("x.jpg").unwrap());
    }
}
