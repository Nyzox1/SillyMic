use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalMessage {
    Hello {
        #[serde(rename = "deviceName")]
        device_name: String,
        #[serde(rename = "appVersion")]
        app_version: String,
        #[serde(rename = "sessionCode")]
        session_code: String,
    },
    Offer {
        sdp: String,
    },
    Answer {
        sdp: String,
    },
    IceCandidate {
        candidate: String,
        #[serde(rename = "sdpMid")]
        sdp_mid: Option<String>,
        #[serde(rename = "sdpMLineIndex")]
        sdp_mline_index: Option<u16>,
    },
    Ready {
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    Error {
        code: String,
        message: String,
    },
}

impl SignalMessage {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }
}
