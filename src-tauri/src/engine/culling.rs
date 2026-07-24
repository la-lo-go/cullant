use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::AppResult;

/// What a culling command applies to. `ids` are file ids; with
/// `as_groups: true` (mirror mode ON) each id expands to every present member
/// of its group — unless the group is decoupled, in which case only the id
/// itself is touched. Fan-out lives in the backend so every caller (keyboard,
/// context menu, future automation) gets identical pair semantics.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Targets {
    pub ids: Vec<i64>,
    #[serde(default)]
    pub as_groups: bool,
}

/// Authoritative post-update state, sent back for UI reconciliation.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CullState {
    pub id: i64,
    pub rating: i64,
    pub flag: i64,
    pub label: Option<String>,
    /// EXIF orientation 1-8. Part of the authoritative row because rotating
    /// changes it, and the frontend keys its image URLs on it.
    pub orientation: i64,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Expand target ids according to group fan-out rules.
pub(crate) fn expand_targets(conn: &Connection, targets: &Targets) -> AppResult<Vec<i64>> {
    if !targets.as_groups {
        return Ok(targets.ids.clone());
    }
    let mut out = Vec::with_capacity(targets.ids.len() * 2);
    let mut stmt = conn.prepare_cached(
        "SELECT m.id FROM files m
         JOIN files f ON f.group_id = m.group_id
         JOIN groups g ON g.id = m.group_id
         WHERE f.id = ?1 AND m.status = 0 AND g.decoupled = 0
         UNION
         SELECT f2.id FROM files f2
         JOIN groups g2 ON g2.id = f2.group_id
         WHERE f2.id = ?1 AND g2.decoupled = 1",
    )?;
    for id in &targets.ids {
        let rows = stmt.query_map(params![id], |r| r.get::<_, i64>(0))?;
        for row in rows {
            out.push(row?);
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn apply<F>(db: &Arc<Db>, targets: Targets, update: F) -> AppResult<Vec<CullState>>
where
    F: Fn(&Connection, i64, i64) -> AppResult<()> + Send + 'static,
{
    db.call(move |conn| {
        let tx = conn.transaction()?;
        let ids = expand_targets(&tx, &targets)?;
        let now = now_secs();
        for id in &ids {
            update(&tx, *id, now)?;
        }
        let mut out = Vec::with_capacity(ids.len());
        {
            let mut stmt = tx.prepare_cached(
                "SELECT id, rating, flag, label, orientation FROM files WHERE id = ?1",
            )?;
            for id in &ids {
                out.push(stmt.query_row(params![id], |r| {
                    Ok(CullState {
                        id: r.get(0)?,
                        rating: r.get(1)?,
                        flag: r.get(2)?,
                        label: r.get(3)?,
                        orientation: r.get::<_, Option<i64>>(4)?.unwrap_or(1),
                    })
                })?);
            }
        }
        tx.commit()?;
        Ok(out)
    })
}

// Only photos (kind 0 = RAW, 1 = image/JPEG) get XMP sidecars; rating,
// flagging or labeling a video must not queue an XMP write.

pub fn set_rating(db: &Arc<Db>, targets: Targets, rating: i64) -> AppResult<Vec<CullState>> {
    let rating = rating.clamp(0, 5);
    apply(db, targets, move |conn, id, now| {
        conn.execute(
            "UPDATE files SET rating = ?2, state_updated_at = ?3,
                   xmp_dirty = CASE WHEN kind IN (0, 1) THEN 1 ELSE xmp_dirty END
             WHERE id = ?1",
            params![id, rating, now],
        )?;
        Ok(())
    })
}

pub fn set_flag(db: &Arc<Db>, targets: Targets, flag: i64) -> AppResult<Vec<CullState>> {
    let flag = flag.clamp(-1, 1);
    apply(db, targets, move |conn, id, now| {
        conn.execute(
            "UPDATE files SET flag = ?2, state_updated_at = ?3,
                   xmp_dirty = CASE WHEN kind IN (0, 1) THEN 1 ELSE xmp_dirty END
             WHERE id = ?1",
            params![id, flag, now],
        )?;
        Ok(())
    })
}

pub fn set_label(
    db: &Arc<Db>,
    targets: Targets,
    label: Option<String>,
) -> AppResult<Vec<CullState>> {
    apply(db, targets, move |conn, id, now| {
        conn.execute(
            "UPDATE files SET label = ?2, state_updated_at = ?3,
                   xmp_dirty = CASE WHEN kind IN (0, 1) THEN 1 ELSE xmp_dirty END
             WHERE id = ?1",
            params![id, label, now],
        )?;
        Ok(())
    })
}

/// Quarter-turn clockwise steps through the EXIF orientation values. There are
/// two independent 4-cycles: unmirrored (1 -> 6 -> 3 -> 8) and mirrored
/// (2 -> 7 -> 4 -> 5). Rotating never crosses between them, so a mirrored scan
/// stays mirrored.
const CW_CYCLES: [[i64; 4]; 2] = [[1, 6, 3, 8], [2, 7, 4, 5]];

/// The orientation reached by turning `from` a quarter turn `steps` times
/// (positive = clockwise). Unknown values fall back to 1 (normal).
fn rotate_orientation(from: i64, steps: i64) -> i64 {
    for cycle in CW_CYCLES {
        if let Some(i) = cycle.iter().position(|&o| o == from) {
            // rem_euclid keeps the index positive for anticlockwise steps.
            return cycle[((i as i64 + steps).rem_euclid(4)) as usize];
        }
    }
    CW_CYCLES[0][(steps.rem_euclid(4)) as usize]
}

/// Turn photos a quarter turn at a time. This is a Cullant-side edit: it
/// rewrites `files.orientation` and never touches the file on disk, so the
/// original bytes stay exactly as the camera wrote them. The rotation reaches
/// other apps through `tiff:Orientation` in the sidecar, hence `xmp_dirty`.
pub fn rotate(db: &Arc<Db>, targets: Targets, steps: i64) -> AppResult<Vec<CullState>> {
    apply(db, targets, move |conn, id, now| {
        let current: i64 = conn
            .query_row(
                "SELECT orientation FROM files WHERE id = ?1",
                params![id],
                |r| r.get::<_, Option<i64>>(0),
            )?
            .unwrap_or(1);
        conn.execute(
            "UPDATE files SET orientation = ?2, state_updated_at = ?3,
                   xmp_dirty = CASE WHEN kind IN (0, 1) THEN 1 ELSE xmp_dirty END
             WHERE id = ?1",
            params![id, rotate_orientation(current, steps), now],
        )?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn rotation_walks_the_exif_cycles() {
        // Unmirrored: a full turn clockwise returns to normal.
        assert_eq!(rotate_orientation(1, 1), 6);
        assert_eq!(rotate_orientation(6, 1), 3);
        assert_eq!(rotate_orientation(3, 1), 8);
        assert_eq!(rotate_orientation(8, 1), 1);
        // Anticlockwise is the exact inverse.
        assert_eq!(rotate_orientation(1, -1), 8);
        assert_eq!(rotate_orientation(6, -1), 1);
        // Mirrored values stay in their own cycle — rotating never un-mirrors.
        assert_eq!(rotate_orientation(2, 1), 7);
        assert_eq!(rotate_orientation(5, 1), 2);
        assert_eq!(rotate_orientation(2, -1), 5);
        // Four quarter turns is a no-op; unknown values normalise to 1's cycle.
        for o in 1..=8 {
            assert_eq!(rotate_orientation(o, 4), o);
        }
        assert_eq!(rotate_orientation(0, 0), 1);
        assert_eq!(rotate_orientation(99, 1), 6);
    }

    fn setup_pair(root: &Path) -> (Arc<Db>, i64, i64) {
        fs::write(root.join("IMG_001.cr3"), b"raw").unwrap();
        fs::write(root.join("IMG_001.jpg"), b"jpg").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();
        let (raw_id, jpg_id) = db
            .call(|c| {
                let raw: i64 =
                    c.query_row("SELECT id FROM files WHERE ext = 'cr3'", [], |r| r.get(0))?;
                let jpg: i64 =
                    c.query_row("SELECT id FROM files WHERE ext = 'jpg'", [], |r| r.get(0))?;
                Ok((raw, jpg))
            })
            .unwrap();
        (db, raw_id, jpg_id)
    }

    #[test]
    fn group_fanout_rates_both_members() {
        let dir = tempfile::tempdir().unwrap();
        let (db, raw_id, jpg_id) = setup_pair(dir.path());

        let updated = set_rating(
            &db,
            Targets {
                ids: vec![raw_id],
                as_groups: true,
            },
            4,
        )
        .unwrap();

        assert_eq!(updated.len(), 2);
        assert!(updated.iter().all(|s| s.rating == 4));
        assert!(updated.iter().any(|s| s.id == jpg_id));
    }

    #[test]
    fn per_file_mode_touches_only_target() {
        let dir = tempfile::tempdir().unwrap();
        let (db, raw_id, jpg_id) = setup_pair(dir.path());

        let updated = set_flag(
            &db,
            Targets {
                ids: vec![jpg_id],
                as_groups: false,
            },
            -1,
        )
        .unwrap();

        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].id, jpg_id);
        let raw_flag: i64 = db
            .call(move |c| {
                Ok(c.query_row(
                    "SELECT flag FROM files WHERE id = ?1",
                    params![raw_id],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(raw_flag, 0);
    }

    #[test]
    fn decoupled_group_does_not_fan_out() {
        let dir = tempfile::tempdir().unwrap();
        let (db, raw_id, jpg_id) = setup_pair(dir.path());

        db.call(|c| {
            c.execute("UPDATE groups SET decoupled = 1", [])?;
            Ok(())
        })
        .unwrap();

        let updated = set_rating(
            &db,
            Targets {
                ids: vec![raw_id],
                as_groups: true,
            },
            5,
        )
        .unwrap();

        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].id, raw_id);
        let jpg_rating: i64 = db
            .call(move |c| {
                Ok(c.query_row(
                    "SELECT rating FROM files WHERE id = ?1",
                    params![jpg_id],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(jpg_rating, 0);
    }
}
