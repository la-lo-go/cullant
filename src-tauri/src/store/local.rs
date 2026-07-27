//! Real-filesystem [`ProjectStore`] — desktop projects and the private
//! `.cullant` sidecar on every platform. A thin wrapper over `std::fs` /
//! `walkdir` that preserves the exact behavior the app had before the storage
//! abstraction existed.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use walkdir::WalkDir;

use super::{split_parent, ProjectStore, StoreEntry, WALK_PROGRESS_EVERY};
use crate::error::AppResult;

pub struct LocalFsStore {
    root: PathBuf,
}

impl LocalFsStore {
    pub fn new(root: impl Into<PathBuf>) -> LocalFsStore {
        LocalFsStore { root: root.into() }
    }

    fn abs(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

fn unix_secs(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl ProjectStore for LocalFsStore {
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
        Ok(std::fs::File::open(self.abs(rel))?)
    }

    fn open_write(&self, rel: &str, _mime_type: &str) -> AppResult<std::fs::File> {
        Ok(std::fs::File::create(self.abs(rel))?)
    }

    fn create_dir_all(&self, rel: &str) -> AppResult<()> {
        std::fs::create_dir_all(self.abs(rel))?;
        Ok(())
    }

    fn rename_in_place(&self, rel: &str, new_name: &str) -> AppResult<String> {
        let (parent, _) = split_parent(rel);
        let new_rel = if parent.is_empty() {
            new_name.to_string()
        } else {
            format!("{parent}/{new_name}")
        };
        std::fs::rename(self.abs(rel), self.abs(&new_rel))?;
        Ok(new_rel)
    }

    fn move_to(&self, rel: &str, new_parent_rel: &str) -> AppResult<String> {
        let (_, name) = split_parent(rel);
        let new_rel = if new_parent_rel.is_empty() {
            name.to_string()
        } else {
            format!("{new_parent_rel}/{name}")
        };
        std::fs::rename(self.abs(rel), self.abs(&new_rel))?;
        Ok(new_rel)
    }

    fn copy(&self, from_rel: &str, to_rel: &str) -> AppResult<()> {
        std::fs::copy(self.abs(from_rel), self.abs(to_rel))?;
        Ok(())
    }

    fn remove_file(&self, rel: &str) -> AppResult<()> {
        std::fs::remove_file(self.abs(rel))?;
        Ok(())
    }

    fn exists(&self, rel: &str) -> AppResult<bool> {
        Ok(self.abs(rel).exists())
    }

    fn local_path(&self, rel: &str) -> Option<PathBuf> {
        Some(self.abs(rel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
