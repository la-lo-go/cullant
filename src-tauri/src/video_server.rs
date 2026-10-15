//! A private HTTP server on the loopback interface, for video playback on
//! Android.
//!
//! Android WebView cannot serve a custom-scheme request with a byte range: it
//! applies the Range again to the partial body it receives, so the media
//! element gets the wrong bytes. That forced every clip larger than one 8 MB
//! window through the in-app remux, which cannot seek back past what it has
//! released, and which drops the 24-bit PCM audio of camera `.MOV`s because
//! Media Source Extensions do not take it. Over real HTTP the WebView's native
//! pipeline plays the same files directly: hardware decode, sound, and seeking
//! that reads only the ranges it needs. Traced on a Galaxy Tab S10+.
//!
//! It is not a network service. It binds `127.0.0.1` only, so nothing outside
//! the device can reach it; it answers only a random per-session token in the
//! path, so another app on the device cannot guess its way in; and it serves
//! only the videos of the project that is open, by id, read-only. The release
//! build's network security config allows cleartext to `127.0.0.1` and nothing
//! else.

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Opens the video with this id in the open project: the file and its length.
pub type Resolve = dyn Fn(i64, Option<&str>) -> Option<(std::fs::File, u64)> + Send + Sync;

pub struct VideoServer {
    pub port: u16,
    pub token: String,
}

impl VideoServer {
    /// Bind a random port on `127.0.0.1` and serve on background threads.
    pub fn start(token: String, resolve: Arc<Resolve>) -> std::io::Result<VideoServer> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let expected = token.clone();
        std::thread::Builder::new()
            .name("video-server".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let resolve = resolve.clone();
                    let expected = expected.clone();
                    let _ =
                        std::thread::Builder::new()
                            .name("video-conn".into())
                            .spawn(move || {
                                if let Err(error) = serve(stream, &expected, resolve.as_ref()) {
                                    tracing::debug!("video server connection ended: {error}");
                                }
                            });
                }
            })?;
        Ok(VideoServer { port, token })
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/{}", self.port, self.token)
    }
}

/// A token nobody can guess: 128 bits from the kernel's generator.
pub fn random_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

struct Request {
    head: bool,
    path: String,
    query: Option<String>,
    range: Option<String>,
}

fn read_request(stream: &TcpStream) -> std::io::Result<Option<Request>> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    let mut range = None;
    let mut total = line.len();
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        total += n;
        // A request head this large is not a media element asking for bytes.
        if n == 0 || total > 16 * 1024 {
            return Ok(None);
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.trim().eq_ignore_ascii_case("range") {
                range = Some(value.trim().to_string());
            }
        }
    }
    if method != "GET" && method != "HEAD" {
        return Ok(None);
    }
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), Some(query.to_string())),
        None => (target.to_string(), None),
    };
    Ok(Some(Request {
        head: method == "HEAD",
        path,
        query,
        range,
    }))
}

fn respond_status(stream: &mut TcpStream, status: &str, extra: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n{extra}\r\n"
    )
}

/// Constant-time comparison, so the token cannot be found byte by byte.
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    let url = tauri::Url::parse(&format!("http://localhost/?{}", query?)).ok()?;
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

/// One request per connection: the media element opens a new one for each
/// range, and on loopback that costs nothing.
fn serve(mut stream: TcpStream, token: &str, resolve: &Resolve) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let Some(request) = read_request(&stream)? else {
        return respond_status(&mut stream, "400 Bad Request", "");
    };
    let mut segments = request.path.trim_start_matches('/').splitn(2, '/');
    let given = segments.next().unwrap_or("");
    let id = segments.next().and_then(|s| s.parse::<i64>().ok());
    let (true, Some(id)) = (same_token(given, token), id) else {
        return respond_status(&mut stream, "404 Not Found", "");
    };
    let project = query_param(request.query.as_deref(), "p");
    let Some((mut file, len)) = resolve(id, project.as_deref()) else {
        return respond_status(&mut stream, "404 Not Found", "");
    };
    let Some((start, end)) = crate::protocol::video_range(request.range.as_deref(), len, u64::MAX)
    else {
        return respond_status(
            &mut stream,
            "416 Range Not Satisfiable",
            &format!("Content-Range: bytes */{len}\r\n"),
        );
    };
    let count = end - start + 1;
    let status = if request.range.is_some() {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    // Announced as MP4 for the reason the custom scheme gives: Chromium parses
    // QuickTime through its MP4 pipeline but rejects `video/quicktime` unread.
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: video/mp4\r\nAccept-Ranges: bytes\r\n\
         Content-Range: bytes {start}-{end}/{len}\r\nContent-Length: {count}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n"
    )?;
    if request.head {
        return Ok(());
    }
    file.seek(SeekFrom::Start(start))?;
    // Streamed, never buffered whole: a range can be the rest of a 1 GB clip,
    // and the element usually closes the connection long before its end.
    let mut remaining = count;
    let mut buffer = vec![0u8; 256 * 1024];
    while remaining > 0 {
        let want = remaining.min(buffer.len() as u64) as usize;
        let read = file.read(&mut buffer[..want])?;
        if read == 0 {
            break;
        }
        stream.write_all(&buffer[..read])?;
        remaining -= read as u64;
    }
    stream.flush()
}

static SERVER: OnceLock<Option<VideoServer>> = OnceLock::new();

/// The base URL videos are served under, starting the server on first use, or
/// `None` where the platform does not need it (every desktop) or it could not
/// start (the frontend then keeps the custom scheme and its remux).
#[tauri::command]
pub fn video_base_url(app: tauri::AppHandle) -> Option<String> {
    if !cfg!(target_os = "android") {
        let _ = app;
        return None;
    }
    SERVER
        .get_or_init(|| {
            let resolve: Arc<Resolve> = Arc::new(move |id, project| open_video(&app, id, project));
            let started = random_token().and_then(|token| VideoServer::start(token, resolve));
            match started {
                Ok(server) => Some(server),
                Err(error) => {
                    tracing::warn!("video server could not start: {error}");
                    None
                }
            }
        })
        .as_ref()
        .map(VideoServer::base_url)
}

fn open_video(
    app: &tauri::AppHandle,
    id: i64,
    project: Option<&str>,
) -> Option<(std::fs::File, u64)> {
    use tauri::Manager;
    let state = app.state::<crate::AppState>();
    let (db, store) = {
        let guard = state.project.lock().unwrap();
        let project_state = guard.as_ref()?;
        (project_state.db.clone(), project_state.store.clone())
    };
    // A URL minted for another project names a file id that means something
    // else here.
    if project.is_some_and(|root| root != db.project_root()) {
        return None;
    }
    let rel: String = db
        .call_read(move |conn| {
            Ok(conn.query_row(
                "SELECT rel_path FROM files WHERE id = ?1 AND status = 0 AND kind = 2",
                [id],
                |r| r.get(0),
            )?)
        })
        .ok()?;
    let file = store.open_read(&rel).ok()?;
    let len = file.metadata().ok()?.len();
    Some((file, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server_for(path: std::path::PathBuf) -> VideoServer {
        let resolve: Arc<Resolve> = Arc::new(move |id, _| {
            (id == 7).then(|| {
                let file = std::fs::File::open(&path).unwrap();
                let len = file.metadata().unwrap().len();
                (file, len)
            })
        });
        VideoServer::start("secret".into(), resolve).unwrap()
    }

    fn get(port: u16, path: &str, range: Option<&str>) -> (String, Vec<u8>) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let range = range.map(|r| format!("Range: {r}\r\n")).unwrap_or_default();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: x\r\n{range}\r\n").unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (
            String::from_utf8_lossy(&response[..split]).into_owned(),
            response[split + 4..].to_vec(),
        )
    }

    #[test]
    fn serves_the_requested_range_and_nothing_without_the_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.mov");
        let data: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &data).unwrap();
        let server = server_for(path);

        let (head, body) = get(server.port, "/secret/7?v=1", Some("bytes=1000-1999"));
        assert!(head.starts_with("HTTP/1.1 206"), "{head}");
        assert!(
            head.contains("Content-Range: bytes 1000-1999/100000"),
            "{head}"
        );
        assert_eq!(body, data[1000..2000]);

        let (head, body) = get(server.port, "/secret/7", Some("bytes=99000-"));
        assert!(head.starts_with("HTTP/1.1 206"), "{head}");
        assert_eq!(body, data[99000..]);

        let (head, body) = get(server.port, "/secret/7", None);
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert_eq!(body.len(), data.len());

        let (head, _) = get(server.port, "/wrong/7", Some("bytes=0-9"));
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
        let (head, _) = get(server.port, "/secret/8", None);
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
        let (head, _) = get(server.port, "/secret/7", Some("bytes=200000-"));
        assert!(head.starts_with("HTTP/1.1 416"), "{head}");
    }

    #[test]
    fn tokens_compare_whole() {
        assert!(same_token("abc", "abc"));
        assert!(!same_token("abc", "abd"));
        assert!(!same_token("abc", "ab"));
    }
}
