use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    Text,
    Image,
    File,
}

impl MessageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MessageKind::Text => "text",
            MessageKind::Image => "image",
            MessageKind::File => "file",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "text" => Some(MessageKind::Text),
            "image" => Some(MessageKind::Image),
            "file" => Some(MessageKind::File),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub id: i64,
    pub hostname: String,
    pub ip: String,
    pub kind: MessageKind,
    pub content: Option<String>,
    pub filename: Option<String>,
    pub orig_name: Option<String>,
    pub created_at: i64,
    pub recalled: bool,
}

/// Message as sent to clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDto {
    pub id: i64,
    pub sender: String,
    pub hostname: String,
    pub ip: String,
    pub avatar_url: String,
    pub kind: MessageKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orig_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    pub created_at: i64,
    pub recalled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SseEvent {
    NewMessage { message: MessageDto },
    Recalled { id: i64 },
}

#[derive(Debug, Deserialize)]
pub struct PostMessageRequest {
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub hostname: String,
    pub ip: String,
    pub display: String,
    pub avatar_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessagesResponse {
    pub messages: Vec<MessageDto>,
    pub has_more: bool,
}
