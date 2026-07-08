use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, Runtime, UriSchemeResponder};

use crate::error::AppResult;
use crate::thumbs::{ThumbKind, ThumbRequest};
use crate::AppState;

/// Handler for the `cullant://` scheme (served as `http://cullant.localhost/…`
/// on Windows). Routes: /thumb/{id}, /preview/{id} (async via the thumb pool),
/// /test (static smoke check). Pixels never cross base64 IPC.
pub fn handle<R: Runtime>(
    app: &AppHandle<R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let path = request.uri().path().to_owned();
    let mut parts = path.trim_start_matches('/').splitn(2, '/');
    let route = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");

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
            responder.respond(plain(StatusCode::SERVICE_UNAVAILABLE, "no project open".into()));
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
        responder.respond(plain(StatusCode::NOT_FOUND, format!("no such file: {file_id}")));
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
                let e = e.unwrap_or(len.saturating_sub(1)).min(len.saturating_sub(1));
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

fn plain(status: StatusCode, message: String) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(message.into_bytes())
        .unwrap()
}
