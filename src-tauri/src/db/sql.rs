//! SQL fragments about the `files` table that more than one module needs.
//!
//! They live beside the schema rather than beside the decoder: they name
//! `files.kind` and `files.ext`, so a column rename is a change here, not in
//! `decode/`. `decode/` keeps what it owns — which extensions exist and what
//! each one means.

use crate::decode::{IMAGE_EXTS, SECONDARY_RAW_EXTS};

fn value_list(exts: &[&str]) -> String {
    exts.iter()
        .map(|e| format!("'{e}'"))
        .collect::<Vec<_>>()
        .join(",")
}

/// [`IMAGE_EXTS`] as a SQL value list (`'jpg','jpeg',…`), for the queries that
/// look for a sibling to borrow a read from. `kind = 1` does not promise the
/// bytes are readable — an opaque HEIF is an image too — so those queries must
/// name the extensions they can open.
///
/// Built once: one caller sits on the thumbnail path and runs per rendered RAW.
pub fn image_exts() -> &'static str {
    static SQL: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| value_list(IMAGE_EXTS));
    &SQL
}

/// [`SECONDARY_RAW_EXTS`] as a SQL value list.
pub fn secondary_raw_exts() -> &'static str {
    static SQL: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| value_list(SECONDARY_RAW_EXTS));
    &SQL
}

/// [`OPAQUE_IMAGE_EXTS`] as a SQL value list.
pub fn heif_exts() -> &'static str {
    static SQL: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| value_list(crate::decode::OPAQUE_IMAGE_EXTS));
    &SQL
}

/// Predicate for "this row's bytes decode into a frame": any RAW, or an image in
/// a container we can open. Thumbnail pregeneration filters on it, so a library
/// of opaque HEIFs is not read end to end to produce nothing — those cells stay
/// empty until something actually displays them.
///
/// `heif` says whether a HEIF decoder is reachable on this machine, so the
/// answer legitimately differs between machines. It feeds a SELECT and never a
/// write, so nothing persistent depends on it.
pub fn decodable_photo(alias: &str, heif: bool) -> String {
    let mut exts = image_exts().to_string();
    if heif {
        exts.push(',');
        exts.push_str(heif_exts());
    }
    format!("({alias}.kind = 0 OR {alias}.ext IN ({exts}))")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::OPAQUE_IMAGE_EXTS;

    /// A HEIF must stay out of this list even once a decoder exists: it governs
    /// the sibling *borrow*, and borrowing a HEIF to render a RAW's thumbnail
    /// would trade the RAW's own embedded JPEG for a subprocess or a JNI hop.
    #[test]
    fn the_image_list_never_holds_a_heif() {
        let sql = image_exts();
        for ext in IMAGE_EXTS {
            assert!(
                sql.contains(&format!("'{ext}'")),
                "{ext} missing from {sql}"
            );
        }
        for ext in OPAQUE_IMAGE_EXTS {
            assert!(
                !sql.contains(&format!("'{ext}'")),
                "{ext} must not be listed"
            );
        }
    }

    /// Pregeneration must queue a HEIF exactly when something can decode it.
    #[test]
    fn the_pregeneration_predicate_follows_the_decoder() {
        let with = decodable_photo("f", true);
        let without = decodable_photo("f", false);
        for ext in OPAQUE_IMAGE_EXTS {
            assert!(with.contains(&format!("'{ext}'")), "{ext} missing");
            assert!(!without.contains(&format!("'{ext}'")), "{ext} must not be");
        }
        // A RAW and a JPEG are queued either way.
        assert!(without.contains("f.kind = 0"));
        assert!(without.contains("'jpg'"));
    }
}
