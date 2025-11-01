use serde::Serialize;

#[derive(Serialize, Default, Debug, Clone)]
pub struct JsonReport {
    pub strategy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_port: Option<u16>,
    pub applied: bool,
    pub verified: bool,
    pub note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl JsonReport {
    pub fn new(strategy: impl Into<String>) -> Self {
        JsonReport {
            strategy: strategy.into(),
            detected_port: None,
            applied: false,
            verified: false,
            note: String::new(),
            error: None,
        }
    }

    pub fn line(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}
