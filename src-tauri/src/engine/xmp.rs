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

/// The sidecar name belonging to ONE file rather than to its basename:
/// `IMG_001.JPG` -> `IMG_001.JPG.xmp`.
///
/// A RAW+JPEG pair maps to a single `IMG_001.xmp`, which is what Lightroom and
/// Capture One expect and what `sidecar_rel` produces. That only works while the
/// two halves agree. When they do not — the RAW queued for deletion and the JPEG
/// kept, say — one of them has to be exported somewhere else, and this is the
/// form Bridge and exiftool use for non-raw files.
pub fn sidecar_rel_per_file(rel_path: &str) -> String {
    format!("{rel_path}.xmp")
}

/// Write (or update) a sidecar at `sc_rel`, through the storage backend.
/// If a sidecar already exists — e.g. Lightroom stored develop settings in it —
/// only OUR attributes are touched; everything else is preserved where possible.
///
/// The target path is passed in rather than derived, because the caller is the
/// one that knows whether this file shares the basename sidecar with its partner
/// or needs a per-file one (see `sidecar_rel_per_file`).
/// Returns the sidecar's rel_path.
pub fn write_sidecar(
    store: &dyn ProjectStore,
    sc_rel: &str,
    state: &XmpState,
) -> AppResult<String> {
    let sc_rel = sc_rel.to_string();
    // Keep foreign data when reading or merging fails.
    let existing =
        if store.exists(&sc_rel)? {
            Some(String::from_utf8(read_all(store, &sc_rel)?).map_err(|e| {
                AppError::Other(format!("sidecar {sc_rel} is not valid UTF-8: {e}"))
            })?)
        } else {
            None
        };
    let output = match existing {
        Some(existing) => merge_into_existing(&existing, state)?,
        None => fresh_sidecar(state),
    };
    store.write_sidecar(&sc_rel, output.as_bytes())?;
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
    let mut depth = 0usize;
    let mut description_depth = None;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| AppError::Other(format!("xmp parse: {e}")))?;
        match event {
            Event::Eof => break,
            Event::Start(el) if !patched && is_description(&el) => {
                depth += 1;
                description_depth = Some(depth);
                writer.write_event(Event::Start(patch_description(&el, state)?))?;
                patched = true;
            }
            Event::Empty(el) if !patched && is_description(&el) => {
                writer.write_event(Event::Empty(patch_description(&el, state)?))?;
                patched = true;
            }
            Event::Start(el)
                if description_depth == Some(depth) && is_our_property(el.name().as_ref()) =>
            {
                reader
                    .read_to_end(el.name())
                    .map_err(|e| AppError::Other(format!("xmp parse: {e}")))?;
            }
            Event::Empty(el)
                if description_depth == Some(depth) && is_our_property(el.name().as_ref()) => {}
            Event::Start(el) => {
                depth += 1;
                validate_attributes(&el)?;
                writer.write_event(Event::Start(el))?;
            }
            Event::Empty(el) => {
                validate_attributes(&el)?;
                writer.write_event(Event::Empty(el))?;
            }
            Event::End(el) => {
                if description_depth == Some(depth) {
                    description_depth = None;
                }
                depth = depth.saturating_sub(1);
                writer.write_event(Event::End(el))?;
            }
            event => writer.write_event(event)?,
        }
    }
    if !patched || depth != 0 {
        return Err(AppError::Other(
            "sidecar has no complete rdf:Description".into(),
        ));
    }
    String::from_utf8(writer.into_inner()).map_err(|e| AppError::Other(format!("xmp utf8: {e}")))
}

fn validate_attributes(el: &BytesStart) -> AppResult<()> {
    for attr in el.attributes() {
        let attr = attr.map_err(|e| AppError::Other(format!("xmp attribute: {e}")))?;
        attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| AppError::Other(format!("xmp attribute value: {e}")))?;
    }
    Ok(())
}

/// Read the culling state out of a sidecar — the inverse of what the writer
/// puts in. Returns `None` when the document has no `rdf:Description`, which is
/// how a sidecar we cannot make sense of is reported (rather than as an empty
/// state that would then wipe the database).
///
/// Absent properties come back at their neutral value, matching the writer:
/// it omits a rating of 0 and a missing label, so their absence means exactly
/// that.
#[cfg(test)]
pub fn read_sidecar(xml: &str) -> Option<XmpState> {
    read_sidecar_import(xml).map(|import| import.state)
}

#[derive(Clone, Debug)]
pub struct XmpImport {
    pub state: XmpState,
    pub has_orientation: bool,
}

pub fn read_sidecar_import(xml: &str) -> Option<XmpImport> {
    let mut reader = Reader::from_str(xml);
    let mut state = XmpState {
        rating: 0,
        flag: 0,
        label: None,
        orientation: 1,
    };
    let mut saw_description = false;
    let mut depth = 0usize;
    let mut description_depth = None;
    let mut saw_pick = false;
    let mut saw_good = false;
    let mut has_orientation = false;
    loop {
        let event = reader.read_event().ok()?;
        let opens_description = matches!(&event, Event::Start(_));
        if opens_description {
            depth += 1;
        }
        match event {
            Event::Eof => {
                return (saw_description && depth == 0).then_some(XmpImport {
                    state,
                    has_orientation,
                })
            }
            Event::Start(el) | Event::Empty(el) if !saw_description && is_description(&el) => {
                saw_description = true;
                description_depth = opens_description.then_some(depth);
                for attr in el.attributes() {
                    let attr = attr.ok()?;
                    let value = attr
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .ok()?;
                    has_orientation |= apply_property(
                        &mut state,
                        &mut saw_pick,
                        &mut saw_good,
                        attr.key.as_ref(),
                        &value,
                    );
                }
            }
            Event::Start(el)
                if description_depth == depth.checked_sub(1)
                    && is_our_property(el.name().as_ref()) =>
            {
                let text = reader.read_text(el.name()).ok()?;
                let decoded = text.xml_content(quick_xml::XmlVersion::Implicit1_0).ok()?;
                let value = quick_xml::escape::unescape(&decoded).ok()?;
                depth = depth.saturating_sub(1);
                has_orientation |= apply_property(
                    &mut state,
                    &mut saw_pick,
                    &mut saw_good,
                    el.name().as_ref(),
                    &value,
                );
            }
            Event::End(_) => {
                if description_depth == Some(depth) {
                    description_depth = None;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
}

fn apply_property(
    state: &mut XmpState,
    saw_pick: &mut bool,
    saw_good: &mut bool,
    key: &[u8],
    value: &str,
) -> bool {
    let value = value.trim();
    match key {
        b"xmp:Rating" => {
            let rating = value.parse::<i64>().unwrap_or(0);
            state.rating = rating.clamp(0, 5);
            if !*saw_pick && !*saw_good {
                state.flag = if rating < 0 { -1 } else { 0 };
            }
        }
        b"xmp:Label" => state.label = (!value.is_empty()).then(|| value.to_string()),
        b"xmpDM:pick" => {
            if let Ok(pick) = value.parse::<i64>() {
                state.flag = pick.clamp(-1, 1);
                *saw_pick = true;
            }
        }
        b"xmpDM:good" if !*saw_pick => {
            *saw_good = true;
            state.flag = match value {
                "True" | "true" => 1,
                "False" | "false" => -1,
                _ => 0,
            };
        }
        b"tiff:Orientation" => {
            let orientation = value.parse::<i64>().unwrap_or(1);
            state.orientation = if (1..=8).contains(&orientation) {
                orientation
            } else {
                1
            };
            return value
                .parse::<i64>()
                .is_ok_and(|orientation| (1..=8).contains(&orientation));
        }
        _ => {}
    }
    false
}

fn is_our_property(name: &[u8]) -> bool {
    matches!(
        name,
        b"xmp:Rating" | b"xmp:Label" | b"xmpDM:pick" | b"xmpDM:good" | b"tiff:Orientation"
    )
}

fn is_description(el: &BytesStart) -> bool {
    is_description_name(el.name().as_ref())
}

fn is_description_name(name: &[u8]) -> bool {
    name == b"rdf:Description" || name.ends_with(b":Description") || name == b"Description"
}

fn patch_description(el: &BytesStart, state: &XmpState) -> AppResult<BytesStart<'static>> {
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

    for attr in el.attributes() {
        let attr = attr.map_err(|e| AppError::Other(format!("xmp attribute: {e}")))?;
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
    Ok(out.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_regression_bad_foreign_sidecar_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::LocalFsStore::new(dir.path());
        let state = XmpState {
            rating: 4,
            flag: 1,
            label: None,
            orientation: 6,
        };
        for content in [
            b"<not-an-xmp>foreign settings</not-an-xmp>".as_slice(),
            b"<rdf:Description broken",
            &[0xff, 0xfe, 0x81],
        ] {
            std::fs::write(dir.path().join("a.xmp"), content).unwrap();
            assert!(write_sidecar(&store, "a.xmp", &state).is_err());
            assert_eq!(std::fs::read(dir.path().join("a.xmp")).unwrap(), content);
        }
    }

    #[test]
    fn data_regression_element_properties_are_read_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::LocalFsStore::new(dir.path());
        let foreign = r#"<rdf:Description xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:xmpDM="http://ns.adobe.com/xmp/1.0/DynamicMedia/" xmlns:tiff="http://ns.adobe.com/tiff/1.0/"><xmp:Rating>4</xmp:Rating><xmp:Label>Red &amp; Blue</xmp:Label><xmpDM:good>True</xmpDM:good><xmpDM:pick>-1</xmpDM:pick><tiff:Orientation>6</tiff:Orientation><foreign>keep</foreign></rdf:Description>"#;
        std::fs::write(dir.path().join("a.xmp"), foreign).unwrap();
        assert_eq!(
            read_sidecar(foreign).unwrap(),
            XmpState {
                rating: 4,
                flag: -1,
                label: Some("Red & Blue".into()),
                orientation: 6
            }
        );
        let state = XmpState {
            rating: 2,
            flag: 1,
            label: None,
            orientation: 1,
        };
        write_sidecar(&store, "a.xmp", &state).unwrap();
        let written = std::fs::read_to_string(dir.path().join("a.xmp")).unwrap();
        assert_eq!(read_sidecar(&written).unwrap(), state);
        assert!(!written.contains("<xmp:Rating>4"));
        assert!(written.contains("<foreign>keep</foreign>"));
    }

    #[test]
    fn data_regression_invalid_foreign_attribute_is_not_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::LocalFsStore::new(dir.path());
        let foreign = "<rdf:Description broken/>";
        std::fs::write(dir.path().join("a.xmp"), foreign).unwrap();
        let state = XmpState {
            rating: 3,
            flag: 0,
            label: None,
            orientation: 1,
        };
        assert!(write_sidecar(&store, "a.xmp", &state).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.xmp")).unwrap(),
            foreign
        );
    }

    #[test]
    fn data_regression_import_reports_orientation_presence() {
        let absent = read_sidecar_import("<rdf:Description xmp:Rating=\"3\"/>").unwrap();
        assert!(!absent.has_orientation);
        let attribute = read_sidecar_import("<rdf:Description tiff:Orientation=\"6\"/>").unwrap();
        assert!(attribute.has_orientation);
        assert_eq!(attribute.state.orientation, 6);
        let element = read_sidecar_import(
            "<rdf:Description><tiff:Orientation>8</tiff:Orientation></rdf:Description>",
        )
        .unwrap();
        assert!(element.has_orientation);
        assert_eq!(element.state.orientation, 8);
    }

    #[test]
    fn data_regression_negative_rating_is_a_reject_with_flag_precedence() {
        let rejected = read_sidecar("<rdf:Description xmp:Rating=\"-1\"/>").unwrap();
        assert_eq!((rejected.rating, rejected.flag), (0, -1));
        let picked =
            read_sidecar("<rdf:Description xmpDM:good=\"True\" xmp:Rating=\"-1\"/>").unwrap();
        assert_eq!(picked.flag, 1);
        let neutral = read_sidecar("<rdf:Description><xmp:Rating>-1</xmp:Rating><xmpDM:pick>0</xmpDM:pick></rdf:Description>").unwrap();
        assert_eq!(neutral.flag, 0);
    }

    #[cfg(windows)]
    #[test]
    fn data_regression_failed_sidecar_replace_keeps_existing_bytes() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.xmp");
        let state = XmpState {
            rating: 4,
            flag: 1,
            label: None,
            orientation: 6,
        };
        let original = fresh_sidecar(&state);
        std::fs::write(&path, &original).unwrap();
        let _locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let store = crate::store::LocalFsStore::new(dir.path());
        assert!(write_sidecar(&store, "a.xmp", &state).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

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
        let sc_rel = write_sidecar(&store, &sidecar_rel("IMG_1.cr3"), &state).unwrap();
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
        let sc_rel = write_sidecar(&store, &sidecar_rel("IMG_2.cr3"), &state).unwrap();
        let content = std::fs::read_to_string(dir.path().join(&sc_rel)).unwrap();
        assert!(content.contains("tiff:Orientation=\"1\""));
        // Out-of-range values normalise rather than reaching the file.
        let odd = XmpState {
            rating: 0,
            flag: 0,
            label: None,
            orientation: 42,
        };
        write_sidecar(&store, &sidecar_rel("IMG_2.cr3"), &odd).unwrap();
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
