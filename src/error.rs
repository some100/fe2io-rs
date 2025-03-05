use thiserror::Error;

#[derive(Error, Debug)]
pub enum Fe2IoError {
    #[error("WebSocket Error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("Audio Decode Error: {0}")]
    Decoder(#[from] rodio::decoder::DecoderError),
    #[error("Audio Stream Error: {0}")]
    Stream(#[from] rodio::StreamError),
    #[error("Audio Player Error: {0}")]
    Player(#[from] rodio::PlayError),
    #[error("HTTP Request Error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Send Error: {0}")]
    Send(#[from] tokio::sync::mpsc::error::SendError<String>),
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Join Error: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("JSON Error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid response from server: {0}")]
    Invalid(String),
    #[error("Disconnected from server")]
    Reconnect(),
    #[error("No tasks spawned")]
    NoTasks(),
    #[error("Failed to connect after allowed attempts")]
    NoRetry(),
    #[error("Failed to receive inputs")]
    RecvClosed(),
}
