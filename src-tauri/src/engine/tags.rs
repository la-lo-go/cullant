use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::{AppError, AppResult};

use super::culling::Targets;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskTag {
    pub id: i64,
    pub name: String,
    pub shortcut: Option<String>,
    pub scope: i64, // 0=photo 1=video 2=both
    pub color: Option<String>,
    pub sort_order: i64,
    pub builtin: bool,
}

/// Ships with sensible defaults; users edit/extend from the tag editor.
pub const DEFAULT_TAGS: &[(&str, i64, &str)] = &[
    ("Retouch", 0, "#d67ab1"),
    ("Color grade", 2, "#7a9bd6"),
    ("Trim", 1, "#d6b37a"),
    ("Stabilize", 1, "#7ad68e"),
];

pub fn seed_defaults(conn: &Connection) -> AppResult<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM task_tags", [], |r| r.get(0))?;
    if count > 0 {
        return Ok(());
    }
    let mut stmt = conn.prepare(
        "INSERT INTO task_tags (name, scope, color, sort_order, builtin)
         VALUES (?1, ?2, ?3, ?4, 1)",
    )?;
    for (i, (name, scope, color)) in DEFAULT_TAGS.iter().enumerate() {
        stmt.execute(params![name, scope, color, i as i64])?;
    }
    Ok(())
}

pub fn list(db: &Arc<Db>) -> AppResult<Vec<TaskTag>> {
    db.call(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, shortcut, scope, color, sort_order, builtin
             FROM task_tags ORDER BY sort_order, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(TaskTag {
                id: r.get(0)?,
                name: r.get(1)?,
                shortcut: r.get(2)?,
                scope: r.get(3)?,
                color: r.get(4)?,
                sort_order: r.get(5)?,
                builtin: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
}

pub fn create(
    db: &Arc<Db>,
    name: String,
    shortcut: Option<String>,
    scope: i64,
    color: Option<String>,
) -> AppResult<TaskTag> {
    db.call(move |conn| {
        let sort_order: i64 = conn
            .query_row("SELECT COALESCE(MAX(sort_order), -1) + 1 FROM task_tags", [], |r| {
                r.get(0)
            })?;
        conn.execute(
            "INSERT INTO task_tags (name, shortcut, scope, color, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, shortcut, scope, color, sort_order],
        )?;
        let id = conn.last_insert_rowid();
        Ok(TaskTag {
            id,
            name,
            shortcut,
            scope,
            color,
            sort_order,
            builtin: false,
        })
    })
}

pub fn update(db: &Arc<Db>, tag: TaskTag) -> AppResult<()> {
    db.call(move |conn| {
        let n = conn.execute(
            "UPDATE task_tags SET name = ?2, shortcut = ?3, scope = ?4, color = ?5,
             sort_order = ?6 WHERE id = ?1",
            params![tag.id, tag.name, tag.shortcut, tag.scope, tag.color, tag.sort_order],
        )?;
        if n == 0 {
            return Err(AppError::Other(format!("no such tag: {}", tag.id)));
        }
        Ok(())
    })
}

pub fn delete(db: &Arc<Db>, tag_id: i64) -> AppResult<()> {
    db.call(move |conn| {
        conn.execute("DELETE FROM task_tags WHERE id = ?1", params![tag_id])?;
        Ok(())
    })
}

/// Toggle a tag on the targets (with group fan-out). The tag's scope is
/// enforced per file: a video-only tag never lands on photos and vice versa.
/// Returns ids whose tag set changed, with the new state.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TagChange {
    pub file_id: i64,
    pub tag_id: i64,
    pub tagged: bool,
}

pub fn toggle(db: &Arc<Db>, targets: Targets, tag_id: i64) -> AppResult<Vec<TagChange>> {
    db.call(move |conn| {
        let tx = conn.transaction()?;
        let scope: i64 = tx
            .query_row(
                "SELECT scope FROM task_tags WHERE id = ?1",
                params![tag_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| AppError::Other(format!("no such tag: {tag_id}")))?;

        let ids = super::culling::expand_targets(&tx, &targets)?;

        // Toggle semantics on a set: if EVERY eligible target already has the
        // tag, remove it everywhere; otherwise add it where missing.
        let mut eligible = Vec::new();
        {
            let mut kind_stmt =
                tx.prepare_cached("SELECT kind FROM files WHERE id = ?1")?;
            for id in &ids {
                let kind: i64 = kind_stmt.query_row(params![id], |r| r.get(0))?;
                let is_video = kind == 2;
                let ok = match scope {
                    0 => !is_video,
                    1 => is_video,
                    _ => true,
                };
                if ok {
                    eligible.push(*id);
                }
            }
        }

        let all_tagged = {
            let mut has_stmt = tx.prepare_cached(
                "SELECT 1 FROM file_tags WHERE file_id = ?1 AND tag_id = ?2",
            )?;
            let mut all = !eligible.is_empty();
            for id in &eligible {
                if has_stmt
                    .query_row(params![id, tag_id], |_| Ok(()))
                    .optional()?
                    .is_none()
                {
                    all = false;
                    break;
                }
            }
            all
        };

        let mut changes = Vec::new();
        if all_tagged {
            let mut del = tx.prepare_cached(
                "DELETE FROM file_tags WHERE file_id = ?1 AND tag_id = ?2",
            )?;
            for id in &eligible {
                del.execute(params![id, tag_id])?;
                changes.push(TagChange {
                    file_id: *id,
                    tag_id,
                    tagged: false,
                });
            }
        } else {
            let mut ins = tx.prepare_cached(
                "INSERT OR IGNORE INTO file_tags (file_id, tag_id) VALUES (?1, ?2)",
            )?;
            for id in &eligible {
                if ins.execute(params![id, tag_id])? > 0 {
                    changes.push(TagChange {
                        file_id: *id,
                        tag_id,
                        tagged: true,
                    });
                }
            }
        }
        tx.commit()?;
        Ok(changes)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn toggle_respects_scope_and_fans_out() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("A.cr3"), b"raw").unwrap();
        fs::write(root.join("A.jpg"), b"jpg").unwrap();
        fs::write(root.join("clip.mp4"), b"vid").unwrap();
        let db = Arc::new(Db::open(root).unwrap());
        crate::scan::scan_project_inner(&db, root, &mut |_| {}).unwrap();

        let tags = list(&db).unwrap();
        let retouch = tags.iter().find(|t| t.name == "Retouch").unwrap(); // photo-only
        let raw_id: i64 = db
            .call(|c| Ok(c.query_row("SELECT id FROM files WHERE ext='cr3'", [], |r| r.get(0))?))
            .unwrap();
        let vid_id: i64 = db
            .call(|c| Ok(c.query_row("SELECT id FROM files WHERE ext='mp4'", [], |r| r.get(0))?))
            .unwrap();

        // Fan-out: tagging the RAW tags the JPEG too.
        let changes = toggle(
            &db,
            Targets {
                ids: vec![raw_id],
                as_groups: true,
            },
            retouch.id,
        )
        .unwrap();
        assert_eq!(changes.len(), 2);
        assert!(changes.iter().all(|c| c.tagged));

        // Photo-only tag is inert on a video.
        let none = toggle(
            &db,
            Targets {
                ids: vec![vid_id],
                as_groups: false,
            },
            retouch.id,
        )
        .unwrap();
        assert!(none.is_empty());

        // Toggling again removes from both.
        let removed = toggle(
            &db,
            Targets {
                ids: vec![raw_id],
                as_groups: true,
            },
            retouch.id,
        )
        .unwrap();
        assert_eq!(removed.len(), 2);
        assert!(removed.iter().all(|c| !c.tagged));
    }
}
