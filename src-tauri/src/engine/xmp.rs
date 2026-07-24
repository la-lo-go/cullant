use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};

use crate::error::{AppError, AppResult};
use crate::store::{read_all, ProjectStore};

/// Culling state exchanged with one file's sidecar, in either direction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmpState {
    pub rating: i64,
    pub flag: i64, // -1 reject, 0 unflagged, 1 pick
    pub label: Option<String>,
    /// EXIF orientation 1-8, exported as `tiff:Orientation`.
    pub orientation: i64,
}

const NS_XMP: &str = "http://ns.adobe.com/xap/1.0/";
const NS_XMPDM: &str = "http://ns.adobe.com/xmp/1.0/DynamicMedia/";
const NS_TIFF: &str = "http://ns.adobe.com/tiff/1.0/";

/// Orientation is written unconditionally, unlike rating/label which are
/// omitted at their neutral value. A sidecar's job here is to OVERRIDE the
/// orientation embedded in the file, so omitting the normal value would let a
/// reader fall back to the file's own EXIF — exactly wrong for a photo the user
/// rotated back upright.
fn orientation_attr(orientation: i64) -> String {
    let o = if (1..=8).contains(&orientation) {
        orientation
    } else {
        1
    };
    o.to_string()
}

/// Sidecar rel_path convention both Lightroom and Capture One read:
/// `IMG_001.CR3` -> `IMG_001.xmp` (basename swap, not name.ext.xmp). Operates on
/// the `/`-separated rel_path string so it's backend-agnostic.
pub fn sidecar_rel(rel_path: &str) -> String {
    let slash = rel_path.rfind('/');
    match rel_path.rfind('.') {
        // Only an extension if the dot is inside the last path segment.
        Some(i) if slash.is_none_or(|s| i > s) => format!("{}.xmp", &rel_path[..i]),
        _ => format!("{rel_path}.xmp"),
    }
}

/// Write (or update) the sidecar for a RAW file, through the storage backend.
/// If a sidecar already exists — e.g. Lightroom stored develop settings in it —
/// only OUR attributes are touched; everything else is preserved where possible.
/// Returns the sidecar's rel_path.
pub fn write_sidecar(
    store: &dyn ProjectStore,
    rel_path: &str,
    state: &XmpState,
) -> AppResult<String> {
    use std::io::Write;

    let sc_rel = sidecar_rel(rel_path);
    // A missing sidecar is expected (write fresh). Any other case that ends in a
    // fresh overwrite must warn first, so foreign content (e.g. Lightroom develop
    // settings) is never lost silently — including a stat error, an unreadable
    // file, or non-UTF-8 bytes.
    let existing = match store.exists(&sc_rel) {
        Ok(true) => match read_all(store, &sc_rel) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Some(text),
                Err(e) => {
                    tracing::warn!("sidecar {sc_rel} is not valid UTF-8 ({e}); rewriting fresh");
                    None
                }
            },
            Err(e) => {
                tracing::warn!("sidecar {sc_rel} unreadable ({e}); rewriting fresh");
                None
            }
        },
        Ok(false) => None,
        Err(e) => {
            tracing::warn!("cannot stat sidecar {sc_rel} ({e}); a fresh write may overwrite it");
            None
        }
    };
    let output = match existing {
        Some(existing) => merge_into_existing(&existing, state).unwrap_or_else(|e| {
            tracing::warn!("sidecar merge failed for {sc_rel} ({e}); rewriting fresh");
            fresh_sidecar(state)
        }),
        None => fresh_sidecar(state),
    };
    let mut f = store.open_write(&sc_rel, "application/xml")?;
    f.write_all(output.as_bytes())?;
    Ok(sc_rel)
}

fn flag_attrs(flag: i64) -> (Option<&'static str>, Option<&'static str>) {
    match flag {
        1 => (Some("1"), Some("True")),
        -1 => (Some("-1"), Some("False")),
        _ => (Some("0"), None),
    }
}

fn fresh_sidecar(state: &XmpState) -> String {
    let mut attrs = String::new();
    if state.rating != 0 {
        attrs.push_str(&format!("\n    xmp:Rating=\"{}\"", state.rating));
    }
    if let Some(label) = &state.label {
        attrs.push_str(&format!("\n    xmp:Label=\"{}\"", xml_escape(label)));
    }
    let (pick, good) = flag_attrs(state.flag);
    if let Some(pick) = pick {
        attrs.push_str(&format!("\n    xmpDM:pick=\"{pick}\""));
    }
    if let Some(good) = good {
        attrs.push_str(&format!("\n    xmpDM:good=\"{good}\""));
    }
    attrs.push_str(&format!(
        "\n    tiff:Orientation=\"{}\"",
        orientation_attr(state.orientation)
    ));
    format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Cullant\">\n \
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  \
         <rdf:Description rdf:about=\"\"\n    \
         xmlns:xmp=\"{NS_XMP}\"\n    \
         xmlns:xmpDM=\"{NS_XMPDM}\"\n    \
         xmlns:tiff=\"{NS_TIFF}\"{attrs}/>\n \
         </rdf:RDF>\n</x:xmpmeta>\n"
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

/// Patch our attributes on the first rdf:Description element, preserving all
/// other content (crs: develop settings, IPTC, ...).
fn merge_into_existing(existing: &str, state: &XmpState) -> AppResult<String> {
    let mut reader = Reader::from_str(existing);
    let mut writer = Writer::new(Vec::new());
    let mut patched = false;

    loop {
        match reader
            .read_event()
            .map_err(|e| AppError::Other(format!("xmp parse: {e}")))?
        {
            Event::Eof => break,
            Event::Start(el) if !patched && is_description(&el) => {
                writer
                    .write_event(Event::Start(patch_description(&el, state)))
                    .map_err(|e| AppError::Other(format!("xmp write: {e}")))?;
                patched = true;
            }
            Event::Empty(el) if !patched && is_description(&el) => {
                writer
                    .write_event(Event::Empty(patch_description(&el, state)))
                    .map_err(|e| AppError::Other(format!("xmp write: {e}")))?;
                patched = true;
            }
            ev => writer
                .write_event(ev)
                .map_err(|e| AppError::Other(format!("xmp write: {e}")))?,
        }
    }

    if !patched {
        return Err(AppError::Other("no rdf:Description element found".into()));
    }
    String::from_utf8(writer.into_inner()).map_err(|e| AppError::Other(format!("xmp utf8: {e}")))
}

/// Read the culling state out of a sidecar — the inverse of what the writer
/// puts in. Returns `None` when the document has no `rdf:Description`, which is
/// how a sidecar we cannot make sense of is reported (rather than as an empty
/// state that would then wipe the database).
///
/// Absent properties come back at their neutral value, matching the writer:
/// it omits a rating of 0 and a missing label, so their absence means exactly
/// that.
pub fn read_sidecar(xml: &str) -> Option<XmpState> {
    let mut reader = Reader::from_str(xml);
    loop {
        let el = match reader.read_event().ok()? {
            Event::Eof => return None,
            Event::Start(el) | Event::Empty(el) if is_description(&el) => el,
            _ => continue,
        };

        let mut state = XmpState {
            rating: 0,
            flag: 0,
            label: None,
            orientation: 1,
        };
        // xmpDM:pick is authoritative when present; xmpDM:good is the fallback
        // for writers that only emit the boolean.
        let mut saw_pick = false;

        for attr in el.attributes().flatten() {
            let value = attr
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()?
                .into_owned();
            match attr.key.as_ref() {
                b"xmp:Rating" => {
                    // Bridge and FastRawViewer write -1 for "rejected". Cullant
                    // keeps that in the flag, so clamp it out of the rating.
                    state.rating = value.trim().parse::<i64>().unwrap_or(0).clamp(0, 5);
                }
                b"xmp:Label" => {
                    let label = value.trim();
                    if !label.is_empty() {
                        state.label = Some(label.to_string());
                    }
                }
                b"xmpDM:pick" => {
                    if let Ok(pick) = value.trim().parse::<i64>() {
                        state.flag = pick.clamp(-1, 1);
                        saw_pick = true;
                    }
                }
                b"xmpDM:good" if !saw_pick => {
                    state.flag = match value.trim() {
                        "True" | "true" => 1,
                        "False" | "false" => -1,
                        _ => 0,
                    };
                }
                b"tiff:Orientation" => {
                    let o = value.trim().parse::<i64>().unwrap_or(1);
                    state.orientation = if (1..=8).contains(&o) { o } else { 1 };
                }
                _ => {}
            }
        }
        return Some(state);
    }
}

fn is_description(el: &BytesStart) -> bool {
    let name = el.name();
    let local = name.as_ref();
    local == b"rdf:Description" || local.ends_with(b":Description") || local == b"Description"
}

fn patch_description(el: &BytesStart, state: &XmpState) -> BytesStart<'static> {
    let mut out = BytesStart::new(String::from_utf8_lossy(el.name().as_ref()).into_owned());
    let ours = [
        "xmp:Rating".as_bytes(),
        "xmp:Label".as_bytes(),
        "xmpDM:pick".as_bytes(),
        "xmpDM:good".as_bytes(),
        "tiff:Orientation".as_bytes(),
    ];
    let mut has_xmp_ns = false;
    let mut has_dm_ns = false;
    let mut has_tiff_ns = false;

    for attr in el.attributes().flatten() {
        let key = attr.key.as_ref();
        if key == b"xmlns:xmp" {
            has_xmp_ns = true;
        }
        if key == b"xmlns:xmpDM" {
            has_dm_ns = true;
        }
        if key == b"xmlns:tiff" {
            has_tiff_ns = true;
        }
        if ours.contains(&key) {
            continue; // dropped; re-added below with current values
        }
        out.push_attribute(attr);
    }

    if !has_xmp_ns {
        out.push_attribute(("xmlns:xmp", NS_XMP));
    }
    if !has_dm_ns {
        out.push_attribute(("xmlns:xmpDM", NS_XMPDM));
    }
    if !has_tiff_ns {
        out.push_attribute(("xmlns:tiff", NS_TIFF));
    }
    if state.rating != 0 {
        out.push_attribute(("xmp:Rating", state.rating.to_string().as_str()));
    }
    if let Some(label) = &state.label {
        out.push_attribute(("xmp:Label", label.as_str()));
    }
    let (pick, good) = flag_attrs(state.flag);
    if let Some(pick) = pick {
        out.push_attribute(("xmpDM:pick", pick));
    }
    if let Some(good) = good {
        out.push_attribute(("xmpDM:good", good));
    }
    out.push_attribute((
        "tiff:Orientation",
        orientation_attr(state.orientation).as_str(),
    ));
    out.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_sidecar_is_written_and_parses() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("IMG_1.cr3"), b"raw").unwrap();
        let store = crate::store::LocalFsStore::new(dir.path());
        let state = XmpState {
            rating: 4,
            flag: 1,
            label: Some("Red".into()),
            orientation: 6,
        };
        let sc_rel = write_sidecar(&store, "IMG_1.cr3", &state).unwrap();
        assert_eq!(sc_rel, "IMG_1.xmp");
        let content = std::fs::read_to_string(dir.path().join(&sc_rel)).unwrap();
        assert!(content.contains("xmp:Rating=\"4\""));
        assert!(content.contains("xmp:Label=\"Red\""));
        assert!(content.contains("xmpDM:pick=\"1\""));
        assert!(content.contains("xmpDM:good=\"True\""));
        assert!(content.contains("tiff:Orientation=\"6\""));
        assert!(content.contains("xmlns:tiff="));
    }

    #[test]
    fn normal_orientation_is_still_written() {
        // Unlike rating/label, the neutral value must be exported: the sidecar
        // OVERRIDES the file's embedded EXIF, so omitting it would let a reader
        // fall back to the original for a photo the user rotated upright.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("IMG_2.cr3"), b"raw").unwrap();
        let store = crate::store::LocalFsStore::new(dir.path());
        let state = XmpState {
            rating: 0,
            flag: 0,
            label: None,
            orientation: 1,
        };
        let sc_rel = write_sidecar(&store, "IMG_2.cr3", &state).unwrap();
        let content = std::fs::read_to_string(dir.path().join(&sc_rel)).unwrap();
        assert!(content.contains("tiff:Orientation=\"1\""));
        // Out-of-range values normalise rather than reaching the file.
        let odd = XmpState {
            rating: 0,
            flag: 0,
            label: None,
            orientation: 42,
        };
        write_sidecar(&store, "IMG_2.cr3", &odd).unwrap();
        let content = std::fs::read_to_string(dir.path().join(&sc_rel)).unwrap();
        assert!(content.contains("tiff:Orientation=\"1\""));
    }

    #[test]
    fn reads_back_what_it_writes() {
        let original = XmpState {
            rating: 3,
            flag: -1,
            label: Some("Blue".into()),
            orientation: 8,
        };
        let parsed = read_sidecar(&fresh_sidecar(&original)).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn reads_a_foreign_sidecar_and_survives_a_useless_one() {
        // What Bridge/FastRawViewer write: rating -1 for "rejected", and the
        // boolean flag rather than the numeric pick.
        let foreign = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:xmpDM="http://ns.adobe.com/xmp/1.0/DynamicMedia/"
    xmp:Rating="-1"
    xmp:Label="Red"
    xmpDM:good="False"/>
 </rdf:RDF>
</x:xmpmeta>"#;
        let s = read_sidecar(foreign).unwrap();
        // The -1 belongs to the flag; the rating clamps rather than going negative.
        assert_eq!(s.rating, 0);
        assert_eq!(s.flag, -1);
        assert_eq!(s.label.as_deref(), Some("Red"));
        // No tiff:Orientation present: neutral, not a guess.
        assert_eq!(s.orientation, 1);

        // A document with no rdf:Description reads as None, not as blank state —
        // blank state would wipe the database on import.
        assert!(read_sidecar("<x:xmpmeta/>").is_none());
        assert!(read_sidecar("not xml at all").is_none());
    }

    #[test]
    fn pick_wins_over_good_whichever_order_they_appear() {
        let with_both = |attrs: &str| {
            format!(
                r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" {attrs}/></rdf:RDF>"#
            )
        };
        let a = read_sidecar(&with_both(r#"xmpDM:pick="1" xmpDM:good="False""#)).unwrap();
        assert_eq!(a.flag, 1);
        let b = read_sidecar(&with_both(r#"xmpDM:good="False" xmpDM:pick="1""#)).unwrap();
        assert_eq!(b.flag, 1, "attribute order must not decide the flag");
    }

    #[test]
    fn merge_preserves_foreign_content_and_replaces_ours() {
        let existing = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
    xmp:Rating="1"
    tiff:Orientation="1"
    crs:Exposure2012="+0.55">
   <crs:Look><rdf:Description crs:Name="Adobe Color"/></crs:Look>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;
        let merged = merge_into_existing(
            existing,
            &XmpState {
                rating: 5,
                flag: -1,
                label: None,
                orientation: 8,
            },
        )
        .unwrap();
        assert!(merged.contains("crs:Exposure2012=\"+0.55\""));
        assert!(merged.contains("Adobe Color"));
        assert!(merged.contains("xmp:Rating=\"5\""));
        assert!(!merged.contains("xmp:Rating=\"1\""));
        assert!(merged.contains("xmpDM:pick=\"-1\""));
        assert!(merged.contains("xmpDM:good=\"False\""));
        // Ours is replaced, not duplicated, and the existing namespace decl is
        // reused rather than added a second time.
        assert!(merged.contains("tiff:Orientation=\"8\""));
        assert!(!merged.contains("tiff:Orientation=\"1\""));
        assert_eq!(merged.matches("xmlns:tiff").count(), 1);
        // Only the outer Description got patched, not the nested crs one.
        assert_eq!(merged.matches("xmpDM:pick").count(), 1);
    }
}
