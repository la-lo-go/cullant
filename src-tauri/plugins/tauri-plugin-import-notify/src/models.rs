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

/// How an import ended.
#[derive(Clone, Debug)]
pub enum Outcome {
    Done,
    Empty,
    Error(String),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPayload<'a> {
    pub project_name: &'a str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePayload {
    pub phase: Phase,
    pub done: u64,
    pub total: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishPayload<'a> {
    pub outcome: &'static str,
    pub error: Option<&'a str>,
    pub project_name: &'a str,
    pub file_count: u64,
}

#[derive(Deserialize)]
pub struct Ack {}

#[derive(Deserialize)]
pub struct GrantedResponse {
    pub granted: bool,
}
