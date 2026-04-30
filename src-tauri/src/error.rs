use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("yt-dlp failed: {0}")]
    YtDlp(String),
    #[error("ffmpeg failed: {0}")]
    FFmpeg(String),
    #[error("whisper failed: {0}")]
    Whisper(String),
    #[error("invalid YouTube URL")]
    InvalidUrl,
    #[error("model not installed")]
    ModelMissing,
    #[error("cancelled")]
    Cancelled,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("tauri: {0}")]
    Tauri(String),
    #[error("{0}")]
    Other(String),
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        AppError::Tauri(e.to_string())
    }
}

impl From<tauri_plugin_shell::Error> for AppError {
    fn from(e: tauri_plugin_shell::Error) -> Self {
        AppError::Other(e.to_string())
    }
}

// Serialize as a plain string so the frontend gets `e.message === "cancelled"` etc.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
