use serde::{Deserialize, Serialize};

/// One child document returned by a SAF directory listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafEntry {
    /// Opaque SAF document id (stable until the document is renamed/moved).
    pub document_id: String,
    /// Display name (single path segment, not a full path).
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// Last-modified time in milliseconds since the Unix epoch (0 if unknown).
    pub mtime: i64,
}

// ---- command payloads / responses (Rust <-> Kotlin bridge) ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TreePayload {
    pub tree_uri: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenTreeResponse {
    pub tree_uri: String,
    pub root_document_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RootDocumentResponse {
    pub root_document_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccessResponse {
    pub ok: bool,
}

/// Status of the tree URI's backing storage volume (Android `StorageManager`).
/// `mounted` is false when the volume is currently absent (a removed SD card or
/// unplugged USB drive); `description` is the user-visible volume name.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    pub mounted: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub removable: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListChildrenPayload {
    pub tree_uri: String,
    pub parent_document_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListChildrenResponse {
    pub entries: Vec<SafEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FdPayload {
    pub tree_uri: String,
    pub document_id: String,
    /// ParcelFileDescriptor.parseMode string: "r", "w", "wt", "rw", "rwt".
    pub mode: String,
}

#[derive(Deserialize)]
pub(crate) struct FdResponse {
    pub fd: Option<i32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateDocumentPayload {
    pub tree_uri: String,
    pub parent_document_id: String,
    pub mime_type: String,
    pub display_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenameDocumentPayload {
    pub tree_uri: String,
    pub document_id: String,
    pub new_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveDocumentPayload {
    pub tree_uri: String,
    pub document_id: String,
    pub source_parent_document_id: String,
    pub target_parent_document_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CopyDocumentPayload {
    pub tree_uri: String,
    pub document_id: String,
    pub target_parent_document_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteDocumentPayload {
    pub tree_uri: String,
    pub document_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenDocumentPayload {
    pub tree_uri: String,
    pub document_id: String,
    /// Explicit MIME to hand the external app, or `None` to let the SAF
    /// provider report the document's real type.
    pub mime_type: Option<String>,
}

/// Returned by create/rename/move/copy: the resulting document's id
/// (which changes on rename/move for many providers).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentIdResponse {
    pub document_id: String,
}
