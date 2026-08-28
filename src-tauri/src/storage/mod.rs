//! Storage-source probing for remembered projects.
//!
//! Distinguishes three states for a project's folder — present ([`StorageState::Ok`]),
//! on a volume that isn't currently connected ([`StorageState::Disconnected`],
//! e.g. a removed SD card / unplugged drive / unmounted share), or missing while
//! its volume IS present ([`StorageState::NotFound`], deleted/moved/revoked) —
//! and classifies the storage kind for a UI badge with a friendly volume name.
//!
//! Desktop keys off the filesystem path; Android off the SAF `content://` volume
//! id. This first cut is std-only and already separates the three states on
//! Windows (drive-letter presence) and classifies Android volumes. Richer detail
//! is layered in behind `cfg` later:
//!   - Windows: `GetDriveType` + `GetVolumeInformation` for kind + real label.
//!   - Linux: `/proc/mounts` to tell an unmounted volume from a deleted folder.
//!   - Android: `StorageManager.getStorageVolumes()` (JNI) for mounted state,
//!     `getDescription()` (friendly name) and `isRemovable()`.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};

/// Whether a remembered project's folder is reachable, and if not, why.
#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StorageState {
    /// The project folder is reachable.
    Ok,
    /// The volume that contains the project is not connected.
    Disconnected,
    /// The volume is present, but the folder is missing or access was revoked.
    NotFound,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // Some variants are only produced once per-OS detail lands.
pub enum StorageKind {
    Internal,
    Removable,
    Network,
    Unknown,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub state: StorageState,
    pub kind: StorageKind,
    /// A friendly volume name for the UI.
    pub volume_name: Option<String>,
}

impl StorageInfo {
    /// Preserve the former boolean availability contract for existing callers.
    pub fn available(&self) -> bool {
        self.state == StorageState::Ok
    }
}

/// Probe the storage backing a remembered project identifier (a filesystem path
/// on desktop, a SAF `content://` tree URI on Android).
pub fn probe<R: Runtime>(app: &AppHandle<R>, id: &str) -> StorageInfo {
    #[cfg(target_os = "android")]
    if id.starts_with("content://") {
        return probe_saf(app, id);
    }
    let _ = app;
    probe_path(id)
}

fn probe_path(id: &str) -> StorageInfo {
    let (kind, volume_name) = classify(id);
    if std::path::Path::new(id).is_dir() {
        return StorageInfo {
            state: StorageState::Ok,
            kind,
            volume_name,
        };
    }
    let state = if volume_present(id) {
        StorageState::NotFound
    } else {
        StorageState::Disconnected
    };
    StorageInfo {
        state,
        kind,
        volume_name,
    }
}

#[cfg(windows)]
fn windows_drive_root(id: &str) -> Option<String> {
    let b = id.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        Some(format!("{}:\\", b[0] as char))
    } else {
        None
    }
}

#[cfg(windows)]
fn volume_present(id: &str) -> bool {
    match windows_drive_root(id) {
        // A present drive root means the volume is mounted (folder just gone);
        // an absent one means the drive itself is disconnected.
        Some(root) => std::path::Path::new(&root).exists(),
        // UNC / unusual path — fall back to the ancestor heuristic.
        None => ancestor_exists(id),
    }
}

#[cfg(windows)]
fn classify(id: &str) -> (StorageKind, Option<String>) {
    let Some(root) = windows_drive_root(id) else {
        return (StorageKind::Unknown, None);
    };
    let kind = match win::drive_type(&root) {
        2 => StorageKind::Removable,     // DRIVE_REMOVABLE (SD card / USB stick)
        3 => StorageKind::Internal,      // DRIVE_FIXED
        4 => StorageKind::Network,       // DRIVE_REMOTE
        5 | 6 => StorageKind::Removable, // DRIVE_CDROM / DRIVE_RAMDISK
        _ => StorageKind::Unknown,       // 0 unknown, 1 no root dir
    };
    // Prefer the volume label ("DATA (E:)"); fall back to the bare letter ("E:").
    let letter = root.trim_end_matches('\\').to_string();
    let name = match win::volume_label(&root) {
        Some(label) if !label.is_empty() => format!("{label} ({letter})"),
        _ => letter,
    };
    (kind, Some(name))
}

/// Minimal kernel32 FFI for drive type + volume label, avoiding a heavy
/// windows-crate dependency (Rust's std already links kernel32).
#[cfg(windows)]
mod win {
    use std::os::windows::ffi::OsStrExt;

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetDriveTypeW(root: *const u16) -> u32;
        #[allow(clippy::too_many_arguments)]
        fn GetVolumeInformationW(
            root: *const u16,
            vol_name: *mut u16,
            vol_name_size: u32,
            serial: *mut u32,
            max_comp_len: *mut u32,
            fs_flags: *mut u32,
            fs_name: *mut u16,
            fs_name_size: u32,
        ) -> i32;
    }

    pub fn drive_type(root: &str) -> u32 {
        let w = wide(root);
        // Safety: `w` is a NUL-terminated UTF-16 string valid for the call.
        unsafe { GetDriveTypeW(w.as_ptr()) }
    }

    pub fn volume_label(root: &str) -> Option<String> {
        let w = wide(root);
        let mut buf = [0u16; 261]; // MAX_PATH + 1
                                   // Safety: `w` is NUL-terminated; `buf` is writable for `buf.len()` u16s;
                                   // the unused out-params are passed as null, which the API permits.
        let ok = unsafe {
            GetVolumeInformationW(
                w.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        };
        if ok == 0 {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

#[cfg(target_os = "linux")]
fn volume_present(id: &str) -> bool {
    // A missing folder's volume is "present" if a filesystem is currently
    // mounted at (or above) the path. When a removable volume is unmounted its
    // mountpoint drops out of /proc/mounts, so a path under a standard external
    // mount root that no longer has a dedicated mount reads as disconnected.
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut best = "";
    for line in mounts.lines() {
        if let Some(mp) = line.split_whitespace().nth(1) {
            if path_has_prefix(id, mp) && mp.len() > best.len() {
                best = mp;
            }
        }
    }
    if best.is_empty() || best == "/" {
        // Only the root fs covers it: under an external-mount root its dedicated
        // mount is gone (disconnected); anywhere else it's a deleted folder.
        !(id.starts_with("/media/") || id.starts_with("/mnt/") || id.starts_with("/run/media/"))
    } else {
        true
    }
}

#[cfg(target_os = "linux")]
fn path_has_prefix(path: &str, mp: &str) -> bool {
    mp == "/" || path == mp || path.starts_with(&format!("{mp}/"))
}

#[cfg(all(not(windows), not(target_os = "linux")))]
fn volume_present(id: &str) -> bool {
    ancestor_exists(id)
}

#[cfg(not(windows))]
fn classify(_id: &str) -> (StorageKind, Option<String>) {
    // TODO(Linux): parse the mount table (/proc/mounts + /sys/.../removable) for
    // a removable flag and volume label; other desktops fall back to Unknown.
    (StorageKind::Unknown, None)
}

/// True if any ancestor directory of `id` exists — a rough "the volume is still
/// there, the folder was just removed" signal for paths without a drive letter.
/// (Linux uses the more precise /proc/mounts check above instead.)
#[cfg(not(target_os = "linux"))]
fn ancestor_exists(id: &str) -> bool {
    let mut cur = std::path::Path::new(id).parent();
    while let Some(p) = cur {
        if p.as_os_str().is_empty() {
            break;
        }
        if p.exists() {
            return true;
        }
        cur = p.parent();
    }
    false
}

#[cfg(target_os = "android")]
fn probe_saf<R: Runtime>(app: &AppHandle<R>, id: &str) -> StorageInfo {
    use tauri_plugin_saf::SafExt;

    let (uri_kind, uri_name) = saf_volume(id);
    // StorageManager tells us whether the volume is physically present, its real
    // name, and whether it's removable; fall back to URI classification if the
    // query fails.
    let info = app.saf().volume_info(id).ok();

    let kind = match &info {
        Some(i) if i.removable => StorageKind::Removable,
        Some(_) => StorageKind::Internal,
        None => uri_kind,
    };
    let volume_name = info
        .as_ref()
        .and_then(|i| i.description.clone())
        .or(uri_name);

    let mounted = info.as_ref().map(|i| i.mounted).unwrap_or(true);
    let state = if !mounted {
        // Volume physically absent → disconnected (removed SD card / USB).
        StorageState::Disconnected
    } else if app.saf().check_tree_access(id).unwrap_or(false) {
        StorageState::Ok
    } else {
        // Volume present but permission revoked or the folder is gone.
        StorageState::NotFound
    };

    StorageInfo {
        state,
        kind,
        volume_name,
    }
}

#[cfg(target_os = "android")]
/// Classify `primary:` as internal storage and a UUID as removable storage.
fn saf_volume(id: &str) -> (StorageKind, Option<String>) {
    let doc_id = id.rsplit('/').next().unwrap_or(id).to_ascii_lowercase();
    if doc_id.starts_with("primary%3a") || doc_id.starts_with("primary:") {
        (StorageKind::Internal, Some("Internal storage".to_string()))
    } else if doc_id.contains("%3a") || doc_id.contains(':') {
        (StorageKind::Removable, Some("SD card / USB".to_string()))
    } else {
        (StorageKind::Unknown, None)
    }
}
