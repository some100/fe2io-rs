use thiserror::Error;
use tokio::{sync::mpsc::error::SendError, task::JoinError};
use tokio_tungstenite::tungstenite::Error as TungsteniteError;

#[derive(Error, Debug)]
pub enum Fe2IoError {
    #[error("WebSocket Error: {0}")]
    WebSocket(#[from] TungsteniteError),
    #[error("Audio Decode Error: {0}")]
    Decoder(#[from] rodio::decoder::DecoderError),
    #[error("Audio Stream Error: {0}")]
    Stream(#[from] rodio::StreamError),
    #[error("Audio Player Error: {0}")]
    Player(#[from] rodio::PlayError),
    #[error("HTTP Request Error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Send Error: {0}")]
    Send(#[from] SendError<crate::MsgValue>),
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Join Error: {0}")]
    Join(#[from] JoinError),
    #[error("JSON Error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Failed to set log subscriber as global default")]
    Subscriber(#[from] tracing::subscriber::SetGlobalDefaultError),
    #[error("Reconnecting to server due to error: {0}")]
    Reconnect(TungsteniteError),
    #[error("Invalid response from server: {0}")]
    Invalid(String),
    #[error("No tasks spawned")]
    NoTasks,
    #[error("Failed to connect after allowed attempts")]
    NoRetry,
    #[error("Audio receiver channels closed")]
    RecvClosed,
}
