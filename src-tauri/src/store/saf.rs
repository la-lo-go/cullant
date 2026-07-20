//! Android SAF-backed [`ProjectStore`].
//!
//! The project "root" is a `content://` tree URI. SAF addresses everything by
//! opaque `document_id`, not by path, so this store keeps a `rel_path ->
//! document_id` cache in the `saf_documents` table (populated during
//! [`list_recursive`](ProjectStore::list_recursive)) and resolves single paths
//! on demand by walking from the nearest cached ancestor.
//!
//! All the heavy lifting (the actual `DocumentsContract` calls) lives in the
//! `tauri-plugin-saf` Kotlin plugin; this file only orchestrates and caches.

use std::sync::Arc;

use rusqlite::{params, OptionalExtension};
use tauri::AppHandle;
use tauri_plugin_saf::{SafEntry, SafExt};

use super::{split_parent, ProjectStore, StoreEntry, MIME_DIRECTORY};
use crate::db::Db;
use crate::error::{AppError, AppResult};

pub struct SafStore {
    tree_uri: String,
    root_document_id: String,
    app: AppHandle,
    db: Arc<Db>,
}

impl SafStore {
    pub fn new(
        tree_uri: String,
        root_document_id: String,
        app: AppHandle,
        db: Arc<Db>,
    ) -> SafStore {
        SafStore {
            tree_uri,
            root_document_id,
            app,
            db,
        }
    }

    fn saf(&self) -> &tauri_plugin_saf::Saf<tauri::Wry> {
        self.app.saf()
    }

    fn err(e: tauri_plugin_saf::Error) -> AppError {
        AppError::Other(format!("saf: {e}"))
    }

    // ---- rel_path -> document_id cache (saf_documents) ----

    fn cache_get(&self, rel: &str) -> AppResult<Option<String>> {
        let rel = rel.to_string();
        self.db.call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT document_id FROM saf_documents WHERE rel_path = ?1",
                    params![rel],
                    |r| r.get::<_, String>(0),
                )
                .optional()?)
        })
    }

    fn cache_put(&self, rel: &str, doc_id: &str, is_dir: bool) -> AppResult<()> {
        let (rel, doc_id) = (rel.to_string(), doc_id.to_string());
        self.db.call(move |conn| {
            conn.execute(
                "INSERT INTO saf_documents (rel_path, document_id, is_dir) VALUES (?1, ?2, ?3)
                 ON CONFLICT(rel_path) DO UPDATE SET document_id = excluded.document_id,
                                                     is_dir = excluded.is_dir",
                params![rel, doc_id, is_dir as i64],
            )?;
            Ok(())
        })
    }

    fn cache_forget(&self, rel: &str) -> AppResult<()> {
        // Forget the entry and, if it's a directory, everything beneath it.
        let rel = rel.to_string();
        let prefix = format!("{rel}/%");
        self.db.call(move |conn| {
            conn.execute(
                "DELETE FROM saf_documents WHERE rel_path = ?1 OR rel_path LIKE ?2",
                params![rel, prefix],
            )?;
            Ok(())
        })
    }

    /// Resolve a rel_path to its SAF document id, walking (and caching) from the
    /// nearest known ancestor when it isn't already cached. `""` is the root.
    fn resolve(&self, rel: &str) -> AppResult<String> {
        if rel.is_empty() {
            return Ok(self.root_document_id.clone());
        }
        if let Some(doc) = self.cache_get(rel)? {
            return Ok(doc);
        }
        // Walk from the root segment by segment, using the cache as a shortcut.
        let mut parent_doc = self.root_document_id.clone();
        let mut prefix = String::new();
        for segment in rel.split('/') {
            let child_rel = if prefix.is_empty() {
                segment.to_string()
            } else {
                format!("{prefix}/{segment}")
            };
            if let Some(doc) = self.cache_get(&child_rel)? {
                parent_doc = doc;
                prefix = child_rel;
                continue;
            }
            let entries = self
                .saf()
                .list_children(&self.tree_uri, &parent_doc)
                .map_err(Self::err)?;
            let found = entries
                .into_iter()
                .find(|e| e.name == segment)
                .ok_or_else(|| AppError::Other(format!("saf: path not found: {rel}")))?;
            self.cache_put(&child_rel, &found.document_id, found.is_dir)?;
            parent_doc = found.document_id;
            prefix = child_rel;
        }
        Ok(parent_doc)
    }

    fn open_mode(&self, rel: &str, mode: &str) -> AppResult<std::fs::File> {
        let doc = self.resolve(rel)?;
        self.saf()
            .open_file(&self.tree_uri, &doc, mode)
            .map_err(Self::err)
    }
}

impl ProjectStore for SafStore {
    fn list_recursive(&self, skip_dirs: &[&str]) -> AppResult<Vec<StoreEntry>> {
        let mut files = Vec::new();
        // (rel_prefix, document_id) worklist, starting at the root.
        let mut stack: Vec<(String, String)> = vec![(String::new(), self.root_document_id.clone())];

        while let Some((prefix, parent_doc)) = stack.pop() {
            let entries: Vec<SafEntry> = self
                .saf()
                .list_children(&self.tree_uri, &parent_doc)
                .map_err(Self::err)?;
            for e in entries {
                let rel = if prefix.is_empty() {
                    e.name.clone()
                } else {
                    format!("{prefix}/{}", e.name)
                };
                self.cache_put(&rel, &e.document_id, e.is_dir)?;
                if e.is_dir {
                    let skip = skip_dirs.contains(&e.name.as_str()) || e.name.starts_with('.');
                    if !skip {
                        stack.push((rel, e.document_id));
                    }
                } else {
                    files.push(StoreEntry {
                        rel_path: rel,
                        size: e.size as i64,
                        // SAF reports last-modified in milliseconds.
                        mtime: e.mtime / 1000,
                    });
                }
            }
        }
        Ok(files)
    }

    fn open_read(&self, rel: &str) -> AppResult<std::fs::File> {
        self.open_mode(rel, "r")
    }

    fn open_write(&self, rel: &str, mime_type: &str) -> AppResult<std::fs::File> {
        // Ensure the file exists (creating it under its parent if needed), then
        // open truncating for write.
        if !self.exists(rel)? {
            let (parent, name) = split_parent(rel);
            self.create_dir_all(parent)?;
            let parent_doc = self.resolve(parent)?;
            let new_doc = self
                .saf()
                .create_document(&self.tree_uri, &parent_doc, mime_type, name)
                .map_err(Self::err)?;
            self.cache_put(rel, &new_doc, false)?;
        }
        self.open_mode(rel, "wt")
    }

    fn create_dir_all(&self, rel: &str) -> AppResult<()> {
        if rel.is_empty() {
            return Ok(());
        }
        let mut parent_doc = self.root_document_id.clone();
        let mut prefix = String::new();
        for segment in rel.split('/') {
            let child_rel = if prefix.is_empty() {
                segment.to_string()
            } else {
                format!("{prefix}/{segment}")
            };
            let doc = match self.cache_get(&child_rel)? {
                Some(doc) => doc,
                None => {
                    // Look for an existing child first, else create the dir.
                    let existing = self
                        .saf()
                        .list_children(&self.tree_uri, &parent_doc)
                        .map_err(Self::err)?
                        .into_iter()
                        .find(|e| e.name == segment);
                    let doc = match existing {
                        Some(e) => e.document_id,
                        None => self
                            .saf()
                            .create_document(&self.tree_uri, &parent_doc, MIME_DIRECTORY, segment)
                            .map_err(Self::err)?,
                    };
                    self.cache_put(&child_rel, &doc, true)?;
                    doc
                }
            };
            parent_doc = doc;
            prefix = child_rel;
        }
        Ok(())
    }

    fn rename_in_place(&self, rel: &str, new_name: &str) -> AppResult<String> {
        let doc = self.resolve(rel)?;
        let new_doc = self
            .saf()
            .rename_document(&self.tree_uri, &doc, new_name)
            .map_err(Self::err)?;
        let (parent, _) = split_parent(rel);
        let new_rel = if parent.is_empty() {
            new_name.to_string()
        } else {
            format!("{parent}/{new_name}")
        };
        self.cache_forget(rel)?;
        self.cache_put(&new_rel, &new_doc, false)?;
        Ok(new_rel)
    }

    fn move_to(&self, rel: &str, new_parent_rel: &str) -> AppResult<String> {
        let doc = self.resolve(rel)?;
        let (src_parent, name) = split_parent(rel);
        let src_parent_doc = self.resolve(src_parent)?;
        self.create_dir_all(new_parent_rel)?;
        let dst_parent_doc = self.resolve(new_parent_rel)?;
        let new_doc = self
            .saf()
            .move_document(&self.tree_uri, &doc, &src_parent_doc, &dst_parent_doc)
            .map_err(Self::err)?;
        let new_rel = if new_parent_rel.is_empty() {
            name.to_string()
        } else {
            format!("{new_parent_rel}/{name}")
        };
        self.cache_forget(rel)?;
        self.cache_put(&new_rel, &new_doc, false)?;
        Ok(new_rel)
    }

    fn copy(&self, from_rel: &str, to_rel: &str) -> AppResult<()> {
        let doc = self.resolve(from_rel)?;
        let (dst_parent, _name) = split_parent(to_rel);
        self.create_dir_all(dst_parent)?;
        let dst_parent_doc = self.resolve(dst_parent)?;
        let new_doc = self
            .saf()
            .copy_document(&self.tree_uri, &doc, &dst_parent_doc)
            .map_err(Self::err)?;
        // `to_rel` may request a different name than the source; SAF copy keeps
        // the source name, so rename afterwards if they differ.
        let (_, want_name) = split_parent(to_rel);
        let (_, src_name) = split_parent(from_rel);
        if want_name != src_name {
            self.saf()
                .rename_document(&self.tree_uri, &new_doc, want_name)
                .map_err(Self::err)?;
        }
        self.cache_forget(to_rel)?;
        Ok(())
    }

    fn remove_file(&self, rel: &str) -> AppResult<()> {
        let doc = self.resolve(rel)?;
        self.saf()
            .delete_document(&self.tree_uri, &doc)
            .map_err(Self::err)?;
        self.cache_forget(rel)?;
        Ok(())
    }

    fn open_external(&self, rel: &str) -> AppResult<()> {
        let doc = self.resolve(rel)?;
        // Pass no MIME: the SAF provider reports the document's real type via
        // ContentResolver.getType, which is more reliable than guessing from the
        // extension. The Kotlin side fires an ACTION_VIEW chooser granting the
        // picked app temporary read access to the content URI.
        self.saf()
            .open_document(&self.tree_uri, &doc, None)
            .map_err(Self::err)?;
        Ok(())
    }

    fn exists(&self, rel: &str) -> AppResult<bool> {
        if rel.is_empty() {
            return Ok(true);
        }
        if self.cache_get(rel)?.is_some() {
            return Ok(true);
        }
        // Not cached: try to resolve; a "not found" error means it doesn't exist.
        match self.resolve(rel) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}
