use std::path::PathBuf;

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
    let rest = parts.next().unwrap_or("");

    if route == "recent-thumb" {
        respond_recent_thumb(app, responder, rest);
        return;
    }

    match (route, rest.parse::<i64>()) {
        ("thumb", Ok(id)) => respond_thumb(app, responder, id, ThumbKind::Thumb),
        ("preview", Ok(id)) => respond_thumb(app, responder, id, ThumbKind::Preview),
        ("full", Ok(id)) => respond_thumb(app, responder, id, ThumbKind::Full),
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

fn respond_thumb<R: Runtime>(
    app: &AppHandle<R>,
    responder: UriSchemeResponder,
    file_id: i64,
    kind: ThumbKind,
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
    let (db, root) = {
        let guard = state.project.lock().unwrap();
        let Some(project) = guard.as_ref() else {
            responder.respond(plain(
                StatusCode::SERVICE_UNAVAILABLE,
                "no project open".into(),
            ));
            return;
        };
        (project.db.clone(), project.root.clone())
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

    let path = root.join(&rel_path);
    let mime = if rel_path.to_lowercase().ends_with(".mov") {
        "video/quicktime"
    } else {
        "video/mp4"
    };

    let result = (|| -> std::io::Result<Response<Vec<u8>>> {
        let mut file = std::fs::File::open(&path)?;
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

    let root = PathBuf::from(&entry.path);
    let db_path = root.join(".cullant").join("cullant.db");
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
    let cache_abs = root.join(".cullant").join("thumbs").join(cache_rel);
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
