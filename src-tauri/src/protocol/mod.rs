use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, Runtime, UriSchemeResponder};

use crate::commands::recent;
use crate::error::AppResult;
use crate::thumbs::{cache_rel_path, ThumbKind, ThumbRequest};
use crate::AppState;

/// Handler for the `cullant://` scheme (served as `http://cullant.localhost/…`
/// on Windows). Routes: /thumb/{id}, /preview/{id} (async via the thumb pool),
/// /recent-thumb/{index}/{slot} (homepage preview, reads a closed project's
/// own thumb cache directly), /test (static smoke check). Pixels never cross
/// base64 IPC.
pub fn handle<R: Runtime>(
    app: &AppHandle<R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let path = request.uri().path().to_owned();
    let mut parts = path.trim_start_matches('/').splitn(2, '/');
    let route = parts.next().unwrap_or("");
    // Hardening: if a platform folds the query into `.path()` (custom-scheme
    // parsers vary), strip anything from `?` on so the id still parses.
    let rest = parts.next().unwrap_or("");
    let rest = rest.split('?').next().unwrap_or(rest);

    if route == "recent-thumb" {
        respond_recent_thumb(app, responder, rest);
        return;
    }

    // mtime carried by `?v=` (frontend cache-buster == files.mtime). Parsed
    // defensively: absent/garbled on any platform → None → authoritative lookup.
    let known_mtime = query_mtime(request.uri().query());

    match (route, rest.parse::<i64>()) {
        ("thumb", Ok(id)) => respond_thumb(app, responder, id, ThumbKind::Thumb, known_mtime),
        ("preview", Ok(id)) => respond_thumb(app, responder, id, ThumbKind::Preview, known_mtime),
        ("full", Ok(id)) => respond_thumb(app, responder, id, ThumbKind::Full, known_mtime),
        ("video", Ok(id)) => respond_video(app, responder, id, &request),
        ("test", _) => responder.respond(
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "image/png")
                .header(header::CACHE_CONTROL, "no-store")
                .body(include_bytes!("../../icons/128x128.png").to_vec())
                .unwrap(),
        ),
        _ => responder.respond(plain(
            StatusCode::NOT_FOUND,
            format!("no such route: {path}"),
        )),
    }
}

/// Extract the `v=<i64>` mtime from a raw query string (`"v=123&x=y"`), if any.
/// Returns `None` when the query is absent or `v` is missing/unparseable — the
/// caller then falls back to the authoritative DB lookup.
fn query_mtime(query: Option<&str>) -> Option<i64> {
    query?
        .split('&')
        .find_map(|kv| kv.strip_prefix("v="))
        .and_then(|v| v.parse::<i64>().ok())
}

fn respond_thumb<R: Runtime>(
    app: &AppHandle<R>,
    responder: UriSchemeResponder,
    file_id: i64,
    kind: ThumbKind,
    known_mtime: Option<i64>,
) {
    let state = app.state::<AppState>();
    let guard = state.project.lock().unwrap();
    let Some(project) = guard.as_ref() else {
        responder.respond(plain(
            StatusCode::SERVICE_UNAVAILABLE,
            "no project open".into(),
        ));
        return;
    };

    project.thumbs.enqueue(ThumbRequest {
        file_id,
        kind,
        known_mtime,
        respond: Box::new(move |result: AppResult<Vec<u8>>| match result {
            Ok(bytes) => responder.respond(
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "image/jpeg")
                    // URLs carry ?v={mtime}, so aggressive caching is safe.
                    .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                    .body(bytes)
                    .unwrap(),
            ),
            Err(e) => {
                tracing::debug!("thumb {file_id} failed: {e}");
                responder.respond(plain(StatusCode::NOT_FOUND, e.to_string()))
            }
        }),
    });
}

/// Serve video bytes with HTTP Range support (mandatory for `<video>`
/// seeking in WebView2). Responses are capped to 8 MB windows so huge MOV
/// files never land in memory at once — the webview just asks again.
fn respond_video<R: Runtime>(
    app: &AppHandle<R>,
    responder: UriSchemeResponder,
    file_id: i64,
    request: &Request<Vec<u8>>,
) {
    use std::io::{Read, Seek, SeekFrom};

    const WINDOW: u64 = 8 * 1024 * 1024;

    let state = app.state::<AppState>();
    let (db, store) = {
        let guard = state.project.lock().unwrap();
        let Some(project) = guard.as_ref() else {
            responder.respond(plain(
                StatusCode::SERVICE_UNAVAILABLE,
                "no project open".into(),
            ));
            return;
        };
        (project.db.clone(), project.store.clone())
    };

    let rel_path: Result<String, _> = db.call(move |conn| {
        Ok(conn.query_row(
            "SELECT rel_path FROM files WHERE id = ?1 AND status = 0",
            [file_id],
            |r| r.get(0),
        )?)
    });
    let Ok(rel_path) = rel_path else {
        responder.respond(plain(
            StatusCode::NOT_FOUND,
            format!("no such file: {file_id}"),
        ));
        return;
    };

    let mime = if rel_path.to_lowercase().ends_with(".mov") {
        "video/quicktime"
    } else {
        "video/mp4"
    };

    // Real `std::fs::File` on every backend (a detached fd on SAF), so the
    // seekable Range logic below is unchanged.
    let mut file = match store.open_read(&rel_path) {
        Ok(f) => f,
        Err(e) => {
            responder.respond(plain(StatusCode::NOT_FOUND, format!("video open: {e}")));
            return;
        }
    };

    let result = (|| -> std::io::Result<Response<Vec<u8>>> {
        let len = file.metadata()?.len();

        // Parse "Range: bytes=start-end" (end optional).
        let range = request
            .headers()
            .get(header::RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("bytes="))
            .and_then(|v| {
                let (s, e) = v.split_once('-')?;
                let start: u64 = s.parse().ok()?;
                let end: Option<u64> = e.parse().ok();
                Some((start, end))
            });

        let (start, end) = match range {
            Some((s, e)) => {
                let e = e
                    .unwrap_or(len.saturating_sub(1))
                    .min(len.saturating_sub(1));
                (s, e.min(s + WINDOW - 1))
            }
            None => (0, (WINDOW - 1).min(len.saturating_sub(1))),
        };
        if start >= len {
            return Ok(Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Vec::new())
                .unwrap());
        }

        let chunk_len = (end - start + 1) as usize;
        let mut buf = vec![0u8; chunk_len];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut buf)?;

        Ok(Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_TYPE, mime)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
            .header(header::CONTENT_LENGTH, chunk_len.to_string())
            .body(buf)
            .unwrap())
    })();

    match result {
        Ok(resp) => responder.respond(resp),
        Err(e) => responder.respond(plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("video read: {e}"),
        )),
    }
}

/// Serve a preview thumbnail for a homepage recent-project card, identified
/// by its position in the recent list plus a preview slot (0..N). Reads
/// straight from that project's own `.cullant` cache on disk — it need not
/// be the currently open project, so this bypasses AppState/ThumbPool
/// entirely and never generates a thumbnail that doesn't already exist.
fn respond_recent_thumb<R: Runtime>(app: &AppHandle<R>, responder: UriSchemeResponder, rest: &str) {
    let mut nums = rest.splitn(2, '/');
    let (Some(index), Some(slot)) = (
        nums.next().and_then(|s| s.parse::<usize>().ok()),
        nums.next().and_then(|s| s.parse::<usize>().ok()),
    ) else {
        responder.respond(plain(
            StatusCode::BAD_REQUEST,
            "bad recent-thumb route".into(),
        ));
        return;
    };

    let list = recent::load_recent(app);
    let Some(entry) = list.get(index) else {
        responder.respond(plain(
            StatusCode::NOT_FOUND,
            "no such recent project".into(),
        ));
        return;
    };

    let base = match crate::commands::project::project_data_base(app, &entry.path) {
        Ok(b) => b,
        Err(_) => {
            responder.respond(plain(
                StatusCode::NOT_FOUND,
                "no such recent project".into(),
            ));
            return;
        }
    };
    let db_path = base.join(".cullant").join("cullant.db");
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY;
    let candidate = rusqlite::Connection::open_with_flags(&db_path, flags)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT id, mtime FROM files WHERE status = 0 AND kind IN (0, 1)
             ORDER BY id DESC LIMIT 1 OFFSET ?1",
                [slot as i64],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .ok()
        });

    let Some((file_id, mtime)) = candidate else {
        responder.respond(plain(StatusCode::NOT_FOUND, "no thumb available".into()));
        return;
    };

    let cache_rel = cache_rel_path(file_id, mtime, ThumbKind::Thumb);
    let cache_abs = base.join(".cullant").join("thumbs").join(cache_rel);
    match std::fs::read(&cache_abs) {
        Ok(bytes) => responder.respond(
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "image/jpeg")
                .header(header::CACHE_CONTROL, "no-store")
                .body(bytes)
                .unwrap(),
        ),
        Err(_) => responder.respond(plain(StatusCode::NOT_FOUND, "thumb not cached".into())),
    }
}

fn plain(status: StatusCode, message: String) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(message.into_bytes())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::query_mtime;

    #[test]
    fn parses_v_param_defensively() {
        // Present and well-formed.
        assert_eq!(query_mtime(Some("v=123")), Some(123));
        assert_eq!(query_mtime(Some("v=0")), Some(0));
        // Among other params, any order.
        assert_eq!(query_mtime(Some("x=1&v=456")), Some(456));
        assert_eq!(query_mtime(Some("v=789&x=1")), Some(789));
        // Absent query, missing/garbled v, or empty → None (fallback path).
        assert_eq!(query_mtime(None), None);
        assert_eq!(query_mtime(Some("")), None);
        assert_eq!(query_mtime(Some("x=1")), None);
        assert_eq!(query_mtime(Some("v=")), None);
        assert_eq!(query_mtime(Some("v=abc")), None);
    }
}
