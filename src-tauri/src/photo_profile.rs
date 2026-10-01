//! Optional aggregate timings. Nested stages overlap; worker sums are not wall time.
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde::Serialize;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Timing {
    count: u64,
    total_ms: f64,
    max_ms: f64,
}

struct Profile {
    started: Instant,
    stages: Mutex<BTreeMap<&'static str, Timing>>,
    output: Mutex<std::fs::File>,
}

fn profile() -> Option<&'static Profile> {
    static PROFILE: OnceLock<Option<Profile>> = OnceLock::new();
    PROFILE
        .get_or_init(|| {
            let path = std::env::var_os("CULLANT_PHOTO_PROFILE")?;
            let output = match std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)
            {
                Ok(file) => file,
                Err(error) => {
                    tracing::warn!("photo profile disabled: {error}");
                    return None;
                }
            };
            Some(Profile {
                started: Instant::now(),
                stages: Mutex::new(BTreeMap::new()),
                output: Mutex::new(output),
            })
        })
        .as_ref()
}

pub(crate) fn start() -> Option<Instant> {
    profile().map(|_| Instant::now())
}

pub(crate) fn record(stage: &'static str, started: Option<Instant>) {
    let Some(started) = started else { return };
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    if let Some(profile) = profile() {
        if let Ok(mut stages) = profile.stages.lock() {
            let timing = stages.entry(stage).or_default();
            timing.count += 1;
            timing.total_ms += elapsed;
            timing.max_ms = timing.max_ms.max(elapsed);
        }
    }
}

pub(crate) struct Span(&'static str, Option<Instant>);

pub(crate) fn span(stage: &'static str) -> Span {
    Span(stage, start())
}

impl Drop for Span {
    fn drop(&mut self) {
        record(self.0, self.1);
    }
}

pub(crate) fn report(event: &'static str) {
    let Some(profile) = profile() else { return };
    let (Ok(stages), Ok(mut output)) = (profile.stages.lock(), profile.output.lock()) else {
        return;
    };
    let row = serde_json::json!({
        "schema": 1, "event": event, "pid": std::process::id(),
        "elapsedMs": profile.started.elapsed().as_secs_f64() * 1000.0,
        "unixMs": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis(),
        "stages": *stages,
    });
    if let Err(error) = writeln!(output, "{row}") {
        tracing::warn!("photo profile write failed: {error}");
    }
}
