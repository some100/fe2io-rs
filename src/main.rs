use std::io::Cursor;
use std::net::TcpStream;
use clap::Parser;
use rodio::{Sink, Decoder, OutputStream};
use serde::Deserialize;
use tungstenite::{WebSocket, connect, Message};
use tungstenite::stream::MaybeTlsStream;
use thiserror::Error;
use log::{debug, info, warn, error};

/// Lighterweight alternative for fe2.io
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Username of player
    username: String,
    /// Volume of sound on death
    #[arg(short, long, default_value_t = 0.5)]
    volume: f32,
    /// WebSocket server URL to connect to
    #[arg(short, long, default_value = "ws://client.fe2.io:8081")]
    url: String,
    /// Delay for failed connection in seconds
    #[arg(long, default_value_t = 5)]
    delay: u64,
    /// Multiplier for delay in failed connection
    #[arg(long, default_value_t = 2)]
    backoff: u64,
    /// Amount of times allowed to reconnect to server
    #[arg(long, default_value_t = 5)]
    attempts: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Msg {
    msg_type: String,
    audio_url: Option<String>,
    status_type: Option<String>,
}

#[derive(Error, Debug)]
pub enum Fe2IoError {
    #[error("WebSocket Error: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("JSON Error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Audio Decode Error: {0}")]
    Decoder(#[from] rodio::decoder::DecoderError),
    #[error("Audio Stream Error: {0}")]
    Stream(#[from] rodio::StreamError),
    #[error("Audio Player Error: {0}")]
    Player(#[from] rodio::PlayError),
    #[error("HTTP Request Error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Error: {0}")]
    Generic(String),
}

fn main() -> Result<(), Fe2IoError> {
    let env = env_logger::Env::default().filter_or("LOG_LEVEL", "info");
    env_logger::init_from_env(env);

    let args = Args::parse();
    let mut server = match connect_to_server(&args.url, &args.username, args.delay, args.backoff, args.attempts) {
        Ok(server) => server,
        Err(e) => {
            error!("{}", e);
            return Err(e);
        },
    };

    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;

    loop {
        match handle_events(&mut server, &sink, &args) {
            Err(Fe2IoError::WebSocket(_)) => { // if a websocket error happens in general (closed, io error, or already closed) we should probably try reconnecting anyways
                warn!("Lost connection to server, attempting to reconnect");
                server = match connect_to_server(&args.url, &args.username, args.delay, args.backoff, args.attempts) {
                    Ok(server) => server,
                    Err(e) => {
                        error!("{}", e);
                        return Err(e);
                    },
                };
            },
            Err(e) => error!("{}", e),
            _ => (),
        }
    }
}

fn connect_to_server(url: &str, username: &str, delay: u64, backoff: u64, attempts: u64) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, Fe2IoError> {
    let mut delay = delay;
    let mut retries = 1;
    let (mut server, _) = loop {
        match connect(url) {
            Ok(server) => break server,
            Err(_e) => {
                if retries > attempts {
                    return Err(Fe2IoError::Generic("Attempts exhausted, bailing".to_owned()));
                }
                warn!("Failed to connect to server {}, retrying in {} seconds. {}/{}", url, delay, retries, attempts);
                std::thread::sleep(std::time::Duration::from_secs(delay));
                delay = std::cmp::min(delay * backoff, 60);
                retries += 1;
            }
        }
    };
    server.send(Message::Text(username.into()))?;
    info!("Connected to server {} with username {}", url, username);
    Ok(server)
}

fn handle_events(server: &mut WebSocket<MaybeTlsStream<TcpStream>>, sink: &Sink, args: &Args) -> Result<(), Fe2IoError> {
    let response = server.read()?;
    let response_as_text = response.to_text()?;
    let msg: Msg = serde_json::from_str(response_as_text)?;
    match msg.msg_type.as_str() {
        "bgm" => play_audio(msg, &sink)?,
        "gameStatus" => handle_status(msg, &sink, &args)?,
        _ => warn!("Server sent invalid msgType {}", msg.msg_type),
    };
    Ok(())
}

fn play_audio(msg: Msg, sink: &Sink) -> Result<(), Fe2IoError> {
    sink.stop();
    let url = msg.audio_url
        .ok_or(Fe2IoError::Generic("Server sent response of type bgm but no URL was provided".to_owned()))?;
    let audio = reqwest::blocking::get(&url)?;
    let cursor = Cursor::new(audio.bytes()?);
    let source = Decoder::new(cursor)?;
    sink.append(source);
    debug!("Playing audio {}", url);
    Ok(())
}

fn handle_status(msg: Msg, sink: &Sink, args: &Args) -> Result<(), Fe2IoError> {
    let status_type = msg.status_type
        .ok_or(Fe2IoError::Generic("Server sent response of type gameStatus but no status was provided".to_owned()))?;
    match status_type.as_str() {
        "died" => sink.set_volume(args.volume),
        "left" => sink.clear(),
        _ => warn!("Server sent invalid statusType {}", status_type),
    }
    debug!("Set game status to {}", status_type);
    Ok(())
}