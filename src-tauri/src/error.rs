use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("clip not found: {0}")]
    ClipNotFound(uuid::Uuid),
    #[error("invalid edit: {0}")]
    InvalidEdit(String),
    #[error("media error: {0}")]
    Media(String),
    #[error("ffmpeg/ffprobe not found on PATH: {0}")]
    BinaryNotFound(String),
    /// whisper-cli / claude: missing, or ran and failed. The message is shown as is.
    #[error("{0}")]
    Tool(String),
    #[error("export failed: {0}")]
    Export(String),
    #[error("cancelled")]
    Cancelled,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

// Tauri commands need a serializable error; send the message string.
impl Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_human_readable() {
        let id = uuid::Uuid::nil();
        assert_eq!(Error::ClipNotFound(id).to_string(), format!("clip not found: {id}"));
        assert_eq!(Error::InvalidEdit("x".into()).to_string(), "invalid edit: x");
        assert_eq!(Error::Media("m".into()).to_string(), "media error: m");
        assert_eq!(Error::BinaryNotFound("ffmpeg".into()).to_string(), "ffmpeg/ffprobe not found on PATH: ffmpeg");
        assert_eq!(Error::Export("e".into()).to_string(), "export failed: e");
        assert_eq!(Error::Tool("whisper-cli not found".into()).to_string(), "whisper-cli not found");
        assert_eq!(Error::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn io_and_json_errors_convert() {
        let io: Error = std::io::Error::new(std::io::ErrorKind::NotFound, "gone").into();
        assert!(matches!(io, Error::Io(_)));
        assert!(io.to_string().starts_with("io: "));
        let js: Error = serde_json::from_str::<u32>("nope").unwrap_err().into();
        assert!(matches!(js, Error::Json(_)));
        assert!(js.to_string().starts_with("json: "));
    }

    #[test]
    fn serializes_as_plain_string_for_the_frontend() {
        let s = serde_json::to_string(&Error::Cancelled).unwrap();
        assert_eq!(s, "\"cancelled\"");
        let s = serde_json::to_string(&Error::InvalidEdit("too short".into())).unwrap();
        assert_eq!(s, "\"invalid edit: too short\"");
    }
}
