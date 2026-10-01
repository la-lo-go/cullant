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
pub(crate) struct OpenTreePayload {
    pub prefer_removable: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TreePayload {
    pub tree_uri: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenTreeResponse {
    pub tree_uri: Option<String>,
    pub root_document_id: Option<String>,
    #[serde(default)]
    pub cancelled: bool,
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

#[derive(Deserialize)]
pub(crate) struct BooleanResponse {
    pub value: bool,
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
pub(crate) struct DocumentPayload {
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VideoPosterPayload {
    pub tree_uri: String,
    pub document_id: String,
    /// Longest edge the extracted frame may have; 0 for its native size.
    pub max_edge: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VideoPosterResponse {
    /// The frame as a base64 (unwrapped) JPEG. The bridge to Kotlin carries
    /// JSON, so bytes have no other way across.
    pub jpeg_base64: String,
    /// The clip's real display dimensions, which the frame may be scaled down
    /// from.
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    /// Seconds since the Unix epoch.
    pub capture_time: Option<i64>,
    /// Stored pixels; the caller applies rotation for display.
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub video_frame_rate: Option<f64>,
    /// Playback duration in seconds.
    pub video_duration: Option<f64>,
    pub rotation: Option<i32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HeifStillPayload {
    pub tree_uri: String,
    pub document_id: String,
    /// Longest edge the decoded image may have; 0 for its native size.
    pub max_edge: u32,
}

/// Same shape as [`VideoPosterResponse`], and deliberately a separate type: the
/// two commands are free to diverge, and one `width`/`height` pair meaning "the
/// clip" and another meaning "the photo" should not share a name.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HeifStillResponse {
    pub jpeg_base64: String,
    /// The file's real dimensions, which the returned image may be sampled down
    /// from.
    pub width: u32,
    pub height: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HeifSupportedResponse {
    pub supported: bool,
}
