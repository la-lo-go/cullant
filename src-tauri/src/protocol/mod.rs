use tauri::http::{header, Request, Response, StatusCode};

/// Handler for the `cullant://` scheme (served as `http://cullant.localhost/…`
/// on Windows). Routes planned: /thumb/{id}, /preview/{id}, /original/{id},
/// /video/{id} (Range), /poster/{id}. For now only /test, which proves the
/// pixel path works without base64 IPC.
pub fn handle(request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let path = request.uri().path().to_owned();
    match path.as_str() {
        "/test" => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/png")
            .header(header::CACHE_CONTROL, "no-store")
            .body(include_bytes!("../../icons/128x128.png").to_vec())
            .unwrap(),
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(format!("no such route: {path}").into_bytes())
            .unwrap(),
    }
}
