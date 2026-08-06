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

/// Predicate for "this row's bytes decode into a frame": any RAW, or an image in
/// a container we can open. Thumbnail pregeneration filters on it, so a library
/// of opaque HEIFs is not read end to end to produce nothing — those cells stay
/// empty until something actually displays them.
pub fn decodable_photo(alias: &str) -> String {
    format!("({alias}.kind = 0 OR {alias}.ext IN ({}))", image_exts())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::OPAQUE_IMAGE_EXTS;

    /// The list is what keeps a sibling read off an undecodable file, so it must
    /// not drift from the extensions the decoder actually handles.
    #[test]
    fn the_image_list_matches_the_decodable_extensions() {
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
}
