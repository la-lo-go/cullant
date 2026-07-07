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

fn plain(status: StatusCode, message: String) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(message.into_bytes())
        .unwrap()
}
