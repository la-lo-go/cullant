use serde::{Deserialize, Serialize};

/// Import stage shown in the notification text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Finding,
    Metadata,
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

#[derive(Serialize)]
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
