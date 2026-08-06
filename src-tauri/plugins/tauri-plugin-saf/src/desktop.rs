use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::error::{Error, Result};
use crate::models::{SafEntry, VolumeInfo};

/// Desktop stub. SAF is Android-only; every method returns
/// [`Error::Unsupported`]. Desktop builds use the real filesystem instead and
/// never construct a `SafStore`, so these are never called in practice — they
/// exist only so the crate compiles and links on all platforms.
pub struct Saf<R: Runtime>(#[allow(dead_code)] AppHandle<R>);

pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Result<Saf<R>> {
    Ok(Saf(app.clone()))
}

impl<R: Runtime> Saf<R> {
    pub fn open_tree(&self) -> Result<(String, String)> {
        Err(Error::Unsupported)
    }
    pub fn root_document_id(&self, _tree_uri: &str) -> Result<String> {
        Err(Error::Unsupported)
    }
    pub fn check_tree_access(&self, _tree_uri: &str) -> Result<bool> {
        Err(Error::Unsupported)
    }
    pub fn volume_info(&self, _tree_uri: &str) -> Result<VolumeInfo> {
        Err(Error::Unsupported)
    }
    pub fn list_children(&self, _tree_uri: &str, _parent: &str) -> Result<Vec<SafEntry>> {
        Err(Error::Unsupported)
    }
    pub fn open_file(&self, _tree_uri: &str, _doc: &str, _mode: &str) -> Result<std::fs::File> {
        Err(Error::Unsupported)
    }
    pub fn create_document(
        &self,
        _tree_uri: &str,
        _parent: &str,
        _mime: &str,
        _name: &str,
    ) -> Result<String> {
        Err(Error::Unsupported)
    }
    pub fn rename_document(&self, _tree_uri: &str, _doc: &str, _new_name: &str) -> Result<String> {
        Err(Error::Unsupported)
    }
    pub fn move_document(
        &self,
        _tree_uri: &str,
        _doc: &str,
        _src_parent: &str,
        _dst_parent: &str,
    ) -> Result<String> {
        Err(Error::Unsupported)
    }
    pub fn copy_document(&self, _tree_uri: &str, _doc: &str, _dst_parent: &str) -> Result<String> {
        Err(Error::Unsupported)
    }
    pub fn delete_document(&self, _tree_uri: &str, _doc: &str) -> Result<()> {
        Err(Error::Unsupported)
    }
    pub fn video_poster(
        &self,
        _tree_uri: &str,
        _doc: &str,
        _max_edge: u32,
    ) -> Result<(Vec<u8>, u32, u32)> {
        Err(Error::Unsupported)
    }
    pub fn heif_supported(&self) -> Result<bool> {
        Err(Error::Unsupported)
    }
    pub fn heif_still(
        &self,
        _tree_uri: &str,
        _doc: &str,
        _max_edge: u32,
    ) -> Result<(Vec<u8>, u32, u32)> {
        Err(Error::Unsupported)
    }
    pub fn open_document(
        &self,
        _tree_uri: &str,
        _doc: &str,
        _mime_type: Option<&str>,
    ) -> Result<()> {
        Err(Error::Unsupported)
    }
}
