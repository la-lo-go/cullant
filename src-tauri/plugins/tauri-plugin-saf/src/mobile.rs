use base64::Engine;
use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::error::{Error, Result};
use crate::models::*;

const PLUGIN_IDENTIFIER: &str = "app.tauri.saf";

/// Handle to the Android SAF plugin.
pub struct Saf<R: Runtime>(tauri::plugin::PluginHandle<R>);

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> Result<Saf<R>> {
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "SafPlugin")?;
    Ok(Saf(handle))
}

impl<R: Runtime> Saf<R> {
    /// Launch `ACTION_OPEN_DOCUMENT_TREE`, persist read/write permission on the
    /// picked tree, and return `(tree_uri, root_document_id)`. Blocks until the
    /// user picks a folder or cancels.
    pub fn open_tree(&self) -> Result<(String, String)> {
        let res: OpenTreeResponse = self.0.run_mobile_plugin("openTree", ())?;
        Ok((res.tree_uri, res.root_document_id))
    }

    /// Resolve the root document id of a previously-persisted tree URI.
    pub fn root_document_id(&self, tree_uri: &str) -> Result<String> {
        let res: RootDocumentResponse = self.0.run_mobile_plugin(
            "rootDocumentId",
            TreePayload {
                tree_uri: tree_uri.to_string(),
            },
        )?;
        Ok(res.root_document_id)
    }

    /// Whether the persisted permission for `tree_uri` is still granted and the
    /// underlying volume is reachable.
    pub fn check_tree_access(&self, tree_uri: &str) -> Result<bool> {
        let res: AccessResponse = self.0.run_mobile_plugin(
            "checkTreeAccess",
            TreePayload {
                tree_uri: tree_uri.to_string(),
            },
        )?;
        Ok(res.ok)
    }

    /// Query the tree URI's backing volume via Android `StorageManager`:
    /// whether it's currently mounted (present), its user-visible description,
    /// and whether it's removable.
    pub fn volume_info(&self, tree_uri: &str) -> Result<VolumeInfo> {
        let res: VolumeInfo = self.0.run_mobile_plugin(
            "volumeInfo",
            TreePayload {
                tree_uri: tree_uri.to_string(),
            },
        )?;
        Ok(res)
    }

    /// List the direct children of `parent_document_id` within `tree_uri`.
    pub fn list_children(
        &self,
        tree_uri: &str,
        parent_document_id: &str,
    ) -> Result<Vec<SafEntry>> {
        let res: ListChildrenResponse = self.0.run_mobile_plugin(
            "listChildren",
            ListChildrenPayload {
                tree_uri: tree_uri.to_string(),
                parent_document_id: parent_document_id.to_string(),
            },
        )?;
        Ok(res.entries)
    }

    /// Open a real `std::fs::File` for a document via a detached
    /// ParcelFileDescriptor. `mode` is a `ParcelFileDescriptor.parseMode`
    /// string ("r", "w", "wt", "rw", "rwt").
    pub fn open_file(
        &self,
        tree_uri: &str,
        document_id: &str,
        mode: &str,
    ) -> Result<std::fs::File> {
        let res: FdResponse = self.0.run_mobile_plugin(
            "getFileDescriptor",
            FdPayload {
                tree_uri: tree_uri.to_string(),
                document_id: document_id.to_string(),
                mode: mode.to_string(),
            },
        )?;
        match res.fd {
            Some(fd) => Ok(unsafe {
                use std::os::fd::FromRawFd;
                std::fs::File::from_raw_fd(fd)
            }),
            None => Err(Error::NoFileDescriptor(document_id.to_string())),
        }
    }

    /// Create a document (file if `mime_type != "vnd.android.document/directory"`,
    /// else a directory) under `parent_document_id`. Returns the new document id.
    pub fn create_document(
        &self,
        tree_uri: &str,
        parent_document_id: &str,
        mime_type: &str,
        display_name: &str,
    ) -> Result<String> {
        let res: DocumentIdResponse = self.0.run_mobile_plugin(
            "createDocument",
            CreateDocumentPayload {
                tree_uri: tree_uri.to_string(),
                parent_document_id: parent_document_id.to_string(),
                mime_type: mime_type.to_string(),
                display_name: display_name.to_string(),
            },
        )?;
        Ok(res.document_id)
    }

    /// Rename a document in place. Returns the (possibly changed) document id.
    pub fn rename_document(
        &self,
        tree_uri: &str,
        document_id: &str,
        new_name: &str,
    ) -> Result<String> {
        let res: DocumentIdResponse = self.0.run_mobile_plugin(
            "renameDocument",
            RenameDocumentPayload {
                tree_uri: tree_uri.to_string(),
                document_id: document_id.to_string(),
                new_name: new_name.to_string(),
            },
        )?;
        Ok(res.document_id)
    }

    /// Move a document from one parent to another. Returns the new document id.
    pub fn move_document(
        &self,
        tree_uri: &str,
        document_id: &str,
        source_parent_document_id: &str,
        target_parent_document_id: &str,
    ) -> Result<String> {
        let res: DocumentIdResponse = self.0.run_mobile_plugin(
            "moveDocument",
            MoveDocumentPayload {
                tree_uri: tree_uri.to_string(),
                document_id: document_id.to_string(),
                source_parent_document_id: source_parent_document_id.to_string(),
                target_parent_document_id: target_parent_document_id.to_string(),
            },
        )?;
        Ok(res.document_id)
    }

    /// Copy a document into `target_parent_document_id`. Returns the new
    /// document id. The Kotlin side falls back to a manual fd-to-fd copy for
    /// providers that don't support `copyDocument`.
    pub fn copy_document(
        &self,
        tree_uri: &str,
        document_id: &str,
        target_parent_document_id: &str,
    ) -> Result<String> {
        let res: DocumentIdResponse = self.0.run_mobile_plugin(
            "copyDocument",
            CopyDocumentPayload {
                tree_uri: tree_uri.to_string(),
                document_id: document_id.to_string(),
                target_parent_document_id: target_parent_document_id.to_string(),
            },
        )?;
        Ok(res.document_id)
    }

    /// Launch an Android `ACTION_VIEW` chooser so the user can open the document
    /// in an external app (e.g. a video player the in-app WebView can't match).
    /// Passing `mime_type = None` lets the SAF provider report the real type.
    pub fn open_document(
        &self,
        tree_uri: &str,
        document_id: &str,
        mime_type: Option<&str>,
    ) -> Result<()> {
        // Kotlin resolves `{ "ok": true }`; reuse AccessResponse to decode it.
        self.0.run_mobile_plugin::<AccessResponse>(
            "openDocument",
            OpenDocumentPayload {
                tree_uri: tree_uri.to_string(),
                document_id: document_id.to_string(),
                mime_type: mime_type.map(str::to_string),
            },
        )?;
        Ok(())
    }

    /// Extract a poster frame from a video document via Android's
    /// `MediaMetadataRetriever`, scaled so its longest edge is at most
    /// `max_edge` (0 = native size). Returns `(jpeg, display_width,
    /// display_height)`, where the dimensions describe the *clip*, not the
    /// (possibly downscaled) frame.
    pub fn video_poster(
        &self,
        tree_uri: &str,
        document_id: &str,
        max_edge: u32,
    ) -> Result<(Vec<u8>, u32, u32)> {
        let res: VideoPosterResponse = self.0.run_mobile_plugin(
            "videoPoster",
            VideoPosterPayload {
                tree_uri: tree_uri.to_string(),
                document_id: document_id.to_string(),
                max_edge,
            },
        )?;
        let jpeg = base64::engine::general_purpose::STANDARD
            .decode(&res.jpeg_base64)
            .map_err(|e| Error::Other(format!("video poster is not valid base64: {e}")))?;
        Ok((jpeg, res.width, res.height))
    }

    /// Permanently delete a document.
    pub fn delete_document(&self, tree_uri: &str, document_id: &str) -> Result<()> {
        // Kotlin resolves `{ "ok": true }`; reuse AccessResponse to decode it.
        self.0.run_mobile_plugin::<AccessResponse>(
            "deleteDocument",
            DeleteDocumentPayload {
                tree_uri: tree_uri.to_string(),
                document_id: document_id.to_string(),
            },
        )?;
        Ok(())
    }
}
