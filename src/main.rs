use std::io::Cursor;
use tokio::{
    net::TcpStream,
    time::{sleep, Duration},
    sync::mpsc::{self, Sender, Receiver, channel},
    task::{self, JoinSet},
};
use tokio_tungstenite::{
    WebSocketStream,
    connect_async, 
    MaybeTlsStream,
    tungstenite::{self, Message},
};
use futures_util::{StreamExt, SinkExt};
use clap::Parser;
use rodio::{Sink, Decoder, OutputStream};
use serde::Deserialize;
use thiserror::Error;
use reqwest::Client;
use log::{debug, info, warn, error};

/// Lighterweight alternative for fe2.io
#[derive(Parser, Clone)]
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

#[derive(Deserialize, Debug)]
struct Msg {
    #[serde(alias = "msgType")] // fe2io compat
    msg_type: String,
    #[serde(alias = "audioUrl")]
    audio_url: Option<String>,
    #[serde(alias = "statusType")]
    status_type: Option<String>,
}

#[derive(Error, Debug)]
pub enum Fe2IoError {
    #[error("WebSocket Error: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("Audio Decode Error: {0}")]
    Decoder(#[from] rodio::decoder::DecoderError),
    #[error("Audio Stream Error: {0}")]
    Stream(#[from] rodio::StreamError),
    #[error("Audio Player Error: {0}")]
    Player(#[from] rodio::PlayError),
    #[error("HTTP Request Error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Send Error: {0}")]
    Send(#[from] mpsc::error::SendError<String>),
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Join Error: {0}")]
    Join(#[from] task::JoinError),
    #[error("JSON Error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Failed to connect after allowed attempts")]
    NoRetry(),
    #[error("Failed to receive inputs")]
    RecvClosed(),
    #[error("Error: {0}")]
    Generic(String),
}

#[tokio::main]
async fn main() -> Result<(), Fe2IoError> {
    let mut tasks = JoinSet::new();

    let env = env_logger::Env::default().filter_or("LOG_LEVEL", "info");
    env_logger::init_from_env(env);

    let args = Args::parse();

    let server = connect_to_server(&args.url, &args.username, args.delay, args.backoff, args.attempts).await?;

    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;

    let (tx, rx) = channel(4);
    tasks.spawn(audio_loop(sink, rx, args.clone()));
    tasks.spawn(event_loop(server, tx, args));

    tokio::select! {
        _ = wait_for_tasks(&mut tasks) => (),
        _ = tokio::signal::ctrl_c() => {
            warn!("Received interrupt, exiting");
            tasks.shutdown().await;
        }
    }
    Ok(())
}

async fn connect_to_server(url: &str, username: &str, delay: u64, backoff: u64, attempts: u64) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Fe2IoError> {
    let mut delay = delay;
    let mut retries = 1;
    let (mut server, _) = loop {
        match connect_async(url).await {
            Ok(server) => break server,
            Err(tungstenite::Error::Url(e)) => return Err(Fe2IoError::WebSocket(tungstenite::Error::Url(e))), // if url is invalid, dont bother retrying since theres no hope of it working
            Err(e) => {
                warn!("Failed to connect to server {}, retrying in {} seconds. {}/{}", url, delay, retries, attempts);
                debug!("Failed to connect: {}", e);
                delay_reconnect(&mut delay, backoff, attempts, &mut retries).await?;
            }
        }
    };
    server.send(Message::Text(username.into())).await?;
    info!("Connected to server {} with username {}", url, username);
    Ok(server)
}

async fn delay_reconnect(delay: &mut u64, backoff: u64, attempts: u64, retries: &mut u64) -> Result<(), Fe2IoError> {
    if *retries > attempts {
        error!("Failed to connect to server after {} attempts, bailing", attempts);
        return Err(Fe2IoError::NoRetry());
    }
    sleep(Duration::from_secs(*delay)).await;
    *delay = (*delay * backoff).min(60);
    *retries += 1;
    Ok(())
}

async fn reconnect_to_server(args: &Args, e: Fe2IoError) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Fe2IoError> {
    error!("{}", e);
    warn!("Lost connection to server, attempting to reconnect");
    connect_to_server(&args.url, &args.username, args.delay, args.backoff, args.attempts).await
}

async fn audio_loop(sink: Sink, mut rx: Receiver<String>, args: Args) -> Result<(), Fe2IoError> {
    let client = Client::new();
    loop {
        match handle_audio_inputs(&sink, &mut rx, &client, &args).await {
            Err(Fe2IoError::RecvClosed()) => {
                error!("Audio receiver channels closed");
                return Err(Fe2IoError::RecvClosed()); // this is not a recoverable error, so just return from loop
            },
            Err(e) => error!("{}", e),
            _ => (),
        }
    }
}

async fn handle_audio_inputs(sink: &Sink, rx: &mut Receiver<String>, client: &Client, args: &Args) -> Result<(), Fe2IoError> {
    let input = rx.recv().await
        .ok_or(Fe2IoError::RecvClosed())?;
    match input.as_str() {
        "died" => sink.set_volume(args.volume),
        "left" => sink.stop(),
        _ => play_audio(sink, client, &input).await?,
    }
    Ok(())
}

async fn play_audio(sink: &Sink, client: &Client, input: &str) -> Result<(), Fe2IoError> {
    sink.stop();
    let audio = client.get(input)
        .send()
        .await?;
    if !audio.status().is_success() {
        return Err(Fe2IoError::Generic(format!("URL {} returned error status {}", input, audio.status())));
    }
    let cursor = Cursor::new(audio.bytes().await?);
    let source = Decoder::new(cursor)?;
    sink.append(source);
    Ok(())
}

async fn event_loop(mut server: WebSocketStream<MaybeTlsStream<TcpStream>>, tx: Sender<String>, args: Args) -> Result<(), Fe2IoError> {
    loop {
        match handle_events(&mut server, tx.clone()).await {
            Err(Fe2IoError::WebSocket(e)) => server = reconnect_to_server(&args, Fe2IoError::WebSocket(e)).await?, // if a websocket error happens in general (closed, io error, or already closed) we should probably try reconnecting anyways
            Err(e) => error!("{}", e),
            _ => (),
        }
    }
}

async fn handle_events(server: &mut WebSocketStream<MaybeTlsStream<TcpStream>>, tx: Sender<String>) -> Result<(), Fe2IoError> {
    let response = read_server_response(server).await?;
    let msg = parse_server_response(response).await?;
    match_server_response(msg, tx).await?;
    Ok(())
}

async fn read_server_response(server: &mut WebSocketStream<MaybeTlsStream<TcpStream>>) -> Result<String, Fe2IoError> {
    let response = match server.next().await {
        Some(response) => response?,
        None => return Err(Fe2IoError::WebSocket(tungstenite::Error::ConnectionClosed)),
    };
    debug!("Received message {}", response);
    Ok(response.to_text()?.to_owned())
}

async fn parse_server_response(response: String) -> Result<Msg, Fe2IoError> {
    let msg: Msg = serde_json::from_str(&response)?;
    debug!("Parsed message {:?}", msg);
    Ok(msg)
}

async fn match_server_response(msg: Msg, tx: Sender<String>) -> Result<(), Fe2IoError> {
    match msg.msg_type.as_str() {
        "bgm" => get_audio(msg, tx).await?,
        "gameStatus" => get_status(msg, tx).await?,
        _ => warn!("Server sent invalid msgType {}", msg.msg_type),
    };
    Ok(())
}

async fn get_audio(msg: Msg, tx: Sender<String>) -> Result<(), Fe2IoError> {
    let url = msg.audio_url
        .ok_or(Fe2IoError::Generic("Server sent response of type bgm but no URL was provided".to_owned()))?;
    debug!("Playing audio {}", url);
    tx.send(url).await?;
    Ok(())
}

async fn get_status(msg: Msg, tx: Sender<String>) -> Result<(), Fe2IoError> {
    let status_type = msg.status_type
        .ok_or(Fe2IoError::Generic("Server sent response of type gameStatus but no status was provided".to_owned()))?;
    debug!("Set game status to {}", status_type);
    tx.send(status_type).await?;
    Ok(())
}

async fn wait_for_tasks(tasks: &mut JoinSet<Result<(), Fe2IoError>>) -> Result<(), Fe2IoError> {
    match tasks.join_next().await {
        Some(Err(e)) => { 
            error!("{}", e);
            return Err(Fe2IoError::Join(e));
        },
        None => {
            error!("Somehow, no tasks were spawned"); // something really bad must have happened for this to be the case
            return Err(Fe2IoError::Generic("No tasks spawned".to_owned()));
        },
        _ => warn!("At least one task exited, ending program"),
    }
    Ok(())
}