use serde::{Deserialize, Serialize};

/// Import stage shown in the notification text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Finding,
    Metadata,
    /// The short thumbnail lead that fills the first screens of the grid. Its
    /// total is the lead size, not the library size.
    FirstThumbnails,
    Thumbnails,
    Previews,
    Videos,
}

/// How an import ended. `Done` carries what the project holds.
#[derive(Clone, Debug)]
pub enum Outcome {
    Done { photos: u64, videos: u64 },
    Empty,
    Error(String),
}

/// The channel Kotlin sends to when the user taps Stop.
#[derive(Serialize)]
pub struct StopHandlerPayload {
    pub handler: tauri::ipc::Channel<()>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPayload<'a> {
    pub project_name: &'a str,
}

/// Progress of the whole import. `None` fields mean unknown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overall {
    /// Thousandths of the whole import. `None` shows an indeterminate bar.
    pub permille: Option<u16>,
    /// Whole minutes left. `Some(0)` means less than a minute.
    pub eta_minutes: Option<u32>,
}

/// `done` and `total` count the current phase only.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePayload {
    pub phase: Phase,
    pub done: u64,
    pub total: Option<u64>,
    #[serde(flatten)]
    pub overall: Overall,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishPayload<'a> {
    pub outcome: &'static str,
    pub error: Option<&'a str>,
    pub project_name: &'a str,
    pub photos: u64,
    pub videos: u64,
}

#[derive(Deserialize)]
pub struct Ack {}

#[derive(Deserialize)]
pub struct GrantedResponse {
    pub granted: bool,
}
