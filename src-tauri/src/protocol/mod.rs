use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, Runtime, UriSchemeResponder};

use crate::commands::recent;
use crate::error::AppResult;
use crate::thumbs::{
    memcache, read_cached_jpeg, resolved_cache_rel_path, CacheVersion, ThumbKind, ThumbRequest,
};
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
    // The webview reaches this scheme cross-origin (the page is served from
    // `tauri.localhost`), so anything script fetches from here — the video
    // route, which the in-app remuxer reads with `fetch` — is a CORS request.
    // A `Range` header is not CORS-safelisted, so it is preflighted first.
    if request.method() == tauri::http::Method::OPTIONS {
        responder.respond(
            cors(Response::builder())
                .status(StatusCode::NO_CONTENT)
                .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, OPTIONS")
                .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Range")
                .body(Vec::new())
                .unwrap(),
        );
        return;
    }

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

    // Cache version carried by `?v=` (files.mtime) and `?o=` (files.orientation).
    // Parsed defensively: absent/garbled on any platform → None → authoritative
    // lookup.
    let known = query_version(request.uri().query());

    if matches!(route, "thumb" | "preview" | "full" | "video") {
        if let Some(expected) = query_param(request.uri().query(), "p") {
            let state = app.state::<AppState>();
            let current = state
                .project
                .lock()
                .unwrap()
                .as_ref()
                .map(|project| project.db.project_root());
            if current.as_deref() != Some(expected.as_str()) {
                responder.respond(plain(StatusCode::GONE, "project changed".into()));
                return;
            }
        }
        if route == "preview"
            && query_param(request.uri().query(), "q")
                .and_then(|value| value.parse::<u32>().ok())
                .is_some_and(|edge| edge != crate::thumbs::preview_long_edge())
        {
            responder.respond(plain(StatusCode::GONE, "preview quality changed".into()));
            return;
        }
    }

    match (route, rest.parse::<i64>()) {
        ("thumb", Ok(id)) => respond_thumb(
            app,
            responder,
            id,
            ThumbKind::Thumb,
            known,
            query_param(request.uri().query(), "b"),
        ),
        ("preview", Ok(id)) => respond_thumb(
            app,
            responder,
            id,
            ThumbKind::Preview,
            known,
            query_param(request.uri().query(), "b"),
        ),
        ("full", Ok(id)) => respond_thumb(
            app,
            responder,
            id,
            ThumbKind::Full,
            known,
            query_param(request.uri().query(), "b"),
        ),
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

/// Extract the cache version from a raw query string (`"v=123&o=6"`), if any.
/// Returns `None` when the query is absent or either part is missing or
/// unparseable — the caller then falls back to the authoritative DB lookup.
/// Both parts are required: guessing one would build a cache path that
/// disagrees with what `render_to_cache` wrote, turning every request into a
/// silent miss.
fn query_version(query: Option<&str>) -> Option<CacheVersion> {
    let query = query?;
    let param = |name: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(name))
            .and_then(|v| v.parse::<i64>().ok())
    };
    Some(CacheVersion {
        mtime: param("v=")?,
        orientation: param("o=")?,
    })
}

fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    let url = tauri::Url::parse(&format!("http://localhost/?{}", query?)).ok()?;
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

fn respond_thumb<R: Runtime>(
    app: &AppHandle<R>,
    responder: UriSchemeResponder,
    file_id: i64,
    kind: ThumbKind,
    known: Option<CacheVersion>,
    expected_source: Option<String>,
) {
    let state = app.state::<AppState>();
    let (thumbs, root, db) = {
        let guard = state.project.lock().unwrap();
        let Some(project) = guard.as_ref() else {
            responder.respond(plain(
                StatusCode::SERVICE_UNAVAILABLE,
                "no project open".into(),
            ));
            return;
        };
        (
            project.thumbs.clone(),
            project.root.clone(),
            project.db.clone(),
        )
    };

    if let Some(expected) = expected_source {
        match crate::thumbs::source_version_for(&db, file_id, ThumbKind::Thumb) {
            Ok(source) if expected == format!("{source:016x}") => {}
            Ok(_) => {
                responder.respond(plain(StatusCode::GONE, "source changed".into()));
                return;
            }
            Err(error) => {
                responder.respond(plain(StatusCode::NOT_FOUND, error.to_string()));
                return;
            }
        }
    }

    // Fast path: an already-cached artifact (with a known version) is served
    // straight from disk by THIS handler thread, without queuing a pool worker.
    // This matters most during the background pregeneration pass: every worker is
    // then busy with a multi-second decode, so an interactive loupe preview that
    // is *already cached* would otherwise sit in the queue behind them and take a
    // second to appear. A cache miss falls through to the pool, which generates
    // and caches exactly as before. (temp-file+rename writes make the read
    // race-safe — a cache file is never partially written.)
    if let Some(version) = known {
        let cache_rel = match resolved_cache_rel_path(&db, file_id, version, kind) {
            Ok(path) => path,
            Err(error) => {
                responder.respond(plain(StatusCode::NOT_FOUND, error.to_string()));
                return;
            }
        };
        let cache_abs = root.join(".cullant").join("thumbs").join(&cache_rel);
        let memory_key = cache_abs.to_string_lossy();
        // Memory first. On Android the WebView is forbidden from caching these
        // (wry rewrites the header to `no-store`), so a cell that scrolls out
        // and back asks again from scratch -- and during an import that wait is
        // long enough to leave a hole where a painted thumbnail used to be.
        if let Some(bytes) = memcache::get(&memory_key) {
            responder.respond(jpeg_ok(bytes.as_ref().clone()));
            return;
        }
        if let Some(bytes) = read_cached_jpeg(&cache_abs) {
            // Only grid thumbnails are worth holding: hundreds are on screen at
            // once and each is tens of kilobytes, where a preview is hundreds
            // and is looked at one at a time.
            if kind == ThumbKind::Thumb {
                memcache::put(&memory_key, std::sync::Arc::new(bytes.clone()));
            }
            responder.respond(jpeg_ok(bytes));
            return;
        }
    }

    thumbs.enqueue(ThumbRequest {
        file_id,
        kind,
        known_version: known,
        // An interactive request wants exactly what was asked for.
        also_thumb: false,
        // The fast path above already tried this exact cache entry.
        cache_checked: known.is_some(),
        respond: Box::new(move |result: AppResult<Vec<u8>>| match result {
            Ok(bytes) => responder.respond(jpeg_ok(bytes)),
            Err(e) => {
                tracing::debug!("thumb {file_id} failed: {e}");
                responder.respond(plain(StatusCode::NOT_FOUND, e.to_string()))
            }
        }),
    });
}

/// A cached-image OK response. URLs carry `?v={mtime}`, so aggressive immutable
/// caching in the webview is safe.
fn jpeg_ok(bytes: Vec<u8>) -> Response<Vec<u8>> {
    cors(Response::builder())
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/jpeg")
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .body(bytes)
        .unwrap()
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

    let rel_path: Result<String, _> = db.call_read(move |conn| {
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

    // Every video is announced as MP4, `.mov` included. Chromium can already
    // parse QuickTime — QTFF and MP4 are near-identical containers and it routes
    // both through the same pipeline — but it never registered `video/quicktime`
    // as a playable type, so labelling a .mov honestly makes the element reject
    // it before it ever looks at the bytes. That is what made every .mov fail on
    // Android. Do not "correct" this back.
    let mime = "video/mp4";

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
        // Android WebView applies Range again to intercepted partial bodies.
        // Script reads use the query to leave its own stream seek at zero.
        let explicit_range = query_param(request.uri().query(), "range");
        let range = explicit_range.as_deref().or_else(|| {
            request
                .headers()
                .get(header::RANGE)
                .map(|value| value.to_str().unwrap_or(""))
        });
        let Some((start, end)) = video_range(range, len, WINDOW) else {
            return Ok(cors(Response::builder())
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Vec::new())
                .unwrap());
        };

        let chunk_len = (end - start + 1) as usize;
        let mut buf = vec![0u8; chunk_len];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut buf)?;

        Ok(cors(Response::builder())
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
    // One representative photo per logical group (a RAW+JPEG pair shares a
    // group_id but is one photo), so slots 0/1/2 are three distinct photos.
    // Within a group prefer the member with a valid cached thumbnail, then the
    // lowest id; order groups by their newest member (most-recent-first, as the
    // old `ORDER BY id DESC` did). Serving still never generates a thumbnail.
    let candidate = rusqlite::Connection::open_with_flags(&db_path, flags)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT id, mtime, orientation, cache_path FROM (
                   SELECT f.id AS id, f.mtime AS mtime, f.orientation AS orientation,
                     t.cache_path AS cache_path,
                     ROW_NUMBER() OVER (
                       PARTITION BY f.group_id
                       ORDER BY (t.file_id IS NOT NULL) DESC, f.id ASC
                     ) AS rn,
                     MAX(f.id) OVER (PARTITION BY f.group_id) AS group_newest
                   FROM files f
                   LEFT JOIN thumbnails t
                     ON t.file_id = f.id AND t.kind = 0 AND t.failed = 0
                        AND t.cache_path <> '' AND t.source_mtime = f.mtime
                   WHERE f.status = 0 AND f.kind IN (0, 1)
                 )
                 WHERE rn = 1
                 ORDER BY group_newest DESC
                 LIMIT 1 OFFSET ?1",
                [slot as i64],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Option<i64>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .ok()
        });

    let Some((_file_id, _mtime, _orientation, Some(cache_rel))) = candidate else {
        responder.respond(plain(StatusCode::NOT_FOUND, "no thumb available".into()));
        return;
    };

    let Some(cache_abs) = crate::thumbs::cached_artifact_path(&base, &cache_rel) else {
        responder.respond(plain(StatusCode::NOT_FOUND, "invalid cache path".into()));
        return;
    };
    match read_cached_jpeg(&cache_abs) {
        Some(bytes) => responder.respond(
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "image/jpeg")
                .header(header::CACHE_CONTROL, "no-store")
                .body(bytes)
                .unwrap(),
        ),
        None => responder.respond(plain(StatusCode::NOT_FOUND, "thumb not cached".into())),
    }
}

fn video_range(range: Option<&str>, len: u64, window: u64) -> Option<(u64, u64)> {
    if len == 0 {
        return None;
    }
    let last = len - 1;
    let (start, end) = match range {
        None => (0, last),
        Some(value) => {
            let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
            if start.is_empty() {
                let suffix: u64 = end.parse().ok()?;
                if suffix == 0 {
                    return None;
                }
                (len.saturating_sub(suffix), last)
            } else {
                let start: u64 = start.parse().ok()?;
                let end: u64 = if end.is_empty() {
                    last
                } else {
                    end.parse().ok()?
                };
                (start, end.min(last))
            }
        }
    };
    if start >= len || end < start {
        return None;
    }
    Some((
        start,
        end.min(start.saturating_add(window.saturating_sub(1))),
    ))
}

fn plain(status: StatusCode, message: String) -> Response<Vec<u8>> {
    cors(Response::builder())
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain")
        .header(header::CACHE_CONTROL, "no-store")
        .body(message.into_bytes())
        .unwrap()
}

/// Allow script on the app's own page to read these responses.
///
/// `*` matches what Tauri's own IPC protocol sends, and carries no more risk:
/// the only thing that can reach this scheme is a page already running inside
/// the app, and Cullant never navigates the webview to remote content.
///
/// `Content-Range` has to be exposed by name — it is not one of the handful of
/// response headers CORS reveals by default, and the video reader needs it to
/// learn the file's length.
fn cors(builder: tauri::http::response::Builder) -> tauri::http::response::Builder {
    builder
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "Content-Range")
}

#[cfg(test)]
mod tests {
    use super::{query_version, CacheVersion};

    fn v(mtime: i64, orientation: i64) -> Option<CacheVersion> {
        Some(CacheVersion { mtime, orientation })
    }

    #[test]
    fn media_regression_image_errors_are_not_cached() {
        let response = super::plain(
            tauri::http::StatusCode::NOT_FOUND,
            "image version changed".into(),
        );
        assert_eq!(
            response.headers()[tauri::http::header::CACHE_CONTROL],
            "no-store"
        );
    }

    #[test]
    fn parses_version_params_defensively() {
        assert_eq!(query_version(Some("v=123&o=6")), v(123, 6));
        assert_eq!(query_version(Some("o=1&v=0")), v(0, 1));
        assert_eq!(query_version(Some("x=1&v=456&y=2&o=8")), v(456, 8));
        assert_eq!(query_version(None), None);
        assert_eq!(query_version(Some("")), None);
        assert_eq!(query_version(Some("x=1")), None);
        assert_eq!(query_version(Some("v=123")), None);
        assert_eq!(query_version(Some("o=6")), None);
        assert_eq!(query_version(Some("v=&o=6")), None);
        assert_eq!(query_version(Some("v=abc&o=6")), None);
        assert_eq!(query_version(Some("v=123&o=abc")), None);
    }
}
