use std::{
    io::Cursor,
    net::TcpStream,
    time::Duration,
    thread::{sleep, spawn},
    sync::mpsc::{self, Sender, Receiver, channel},
};
use clap::Parser;
use rodio::{Sink, Decoder, OutputStream};
use serde::Deserialize;
use tungstenite::{
    WebSocket, 
    connect, 
    Message,
    stream::MaybeTlsStream,
};
use thiserror::Error;
use ureq::{agent, Agent};
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
    /// Maximum value of delay for failed connection in seconds
    #[arg(long, default_value_t = 30)]
    max_delay: u64,
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
    type_: String,
    #[serde(alias = "audioUrl")]
    audio_url: Option<String>,
    #[serde(alias = "statusType")]
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
    Http(#[from] ureq::Error),
    #[error("Send Error: {0}")]
    Send(#[from] mpsc::SendError<String>),
    #[error("Recv Error: {0}")]
    Recv(#[from] mpsc::RecvError<>),
    #[error("Invalid response from server: {0}")]
    Invalid(String),
    #[error("Failed to receive inputs")]
    RecvClosed(),
    #[error("Lost connection with server")]
    Reconnect(),
    #[error("Retries exhausted")]
    NoRetry(),
}

fn main() -> Result<(), Fe2IoError> {
    let env = env_logger::Env::default().filter_or("LOG_LEVEL", "info");
    env_logger::init_from_env(env);

    let args = Args::parse();
    let server = connect_to_server(&args)?;

    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;

    let (tx, rx) = channel();
    let args_clone = args.clone();
    spawn(move || {
        audio_loop(&sink, &rx, &args_clone)
    });
    event_loop(server, &tx, &args)?;
    Ok(())
}

fn connect_to_server(args: &Args) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, Fe2IoError> {
    let mut delay = args.delay;
    let mut retries = 1;
    let (mut server, _) = loop {
        match connect(&args.url) {
            Ok(server) => break server,
            Err(tungstenite::Error::Url(e)) => return Err(Fe2IoError::WebSocket(e.into())), // if url is invalid, dont bother retrying since theres no hope of it working
            Err(e) => {
                warn!("Failed to connect to server {}, retrying in {} seconds. {}/{}", args.url, delay, retries, args.attempts);
                debug!("Failed to connect: {}", e);
                (delay, retries) = delay_reconnect(delay, retries, args)?;
            }
        }
    };
    server.send(Message::Text((&args.username).into()))?;
    info!("Connected to server {} with username {}", args.url, args.username);
    Ok(server)
}

fn delay_reconnect(mut delay: u64, mut retries: u64, args: &Args) -> Result<(u64, u64), Fe2IoError> {
    if retries >= args.attempts {
        error!("Failed to connect to server after {} attempts, bailing", args.attempts);
        return Err(Fe2IoError::NoRetry());
    }
    sleep(Duration::from_secs(delay));
    delay = (delay * args.backoff).min(args.max_delay);
    retries += 1;
    Ok((delay, retries))
}

fn reconnect_to_server(args: &Args) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, Fe2IoError> {
    warn!("Lost connection to server, attempting to reconnect");
    connect_to_server(args)
}

fn audio_loop(sink: &Sink, rx: &Receiver<String>, args: &Args) -> Result<(), Fe2IoError> {
    let agent = agent();
    loop {
        match handle_audio_inputs(sink, rx, &agent, args) {
            Err(Fe2IoError::RecvClosed()) => {
                error!("Audio receiver channels closed");
                return Err(Fe2IoError::RecvClosed()); // this is not a recoverable error, so just return from loop
            },
            Err(e) => error!("{}", e),
            _ => (),
        }
    }
}

fn handle_audio_inputs(sink: &Sink, rx: &Receiver<String>, agent: &Agent, args: &Args) -> Result<(), Fe2IoError> {
    let input = rx.recv()?;
    match input.as_str() {
        "died" => sink.set_volume(args.volume),
        "left" => sink.stop(),
        _ => play_audio(sink, agent, &input)?,
    }
    Ok(())
}

fn play_audio(sink: &Sink, agent: &Agent, input: &str) -> Result<(), Fe2IoError> {
    sink.stop();
    let audio = agent
        .get(input)
        .call()?;
    let audio = match audio.status().as_u16() {
        200 => audio.into_body().read_to_vec()?,
        _ => return Err(Fe2IoError::Invalid(format!("URL {} returned error status {}", input, audio.status()))),
    };
    let cursor = Cursor::new(audio);
    let source = Decoder::new(cursor)?;
    sink.append(source);
    Ok(())
}

fn event_loop(mut server: WebSocket<MaybeTlsStream<TcpStream>>, tx: &Sender<String>, args: &Args) -> Result<(), Fe2IoError> {
    loop {
        match handle_events(&mut server, &tx.clone()) {
            Err(Fe2IoError::WebSocket(_)) => server = reconnect_to_server(args)?,
            Err(Fe2IoError::Send(e)) => return Err(Fe2IoError::Send(e)),
            Err(e) => error!("{}", e),
            _ => (),
        }
    }
}
fn handle_events(server: &mut WebSocket<MaybeTlsStream<TcpStream>>, tx: &Sender<String>) -> Result<(), Fe2IoError> {
    let response = read_server_response(server)?;
    let msg = parse_server_response(&response)?;
    match_server_response(msg, tx)?;
    Ok(())
}

fn read_server_response(server: &mut WebSocket<MaybeTlsStream<TcpStream>>) -> Result<String, Fe2IoError> {
    let Ok(response) = server.read() else {
        return Err(Fe2IoError::Reconnect());
    };
    debug!("Received message {}", response);
    Ok(response.to_text()?.to_owned())
}

fn parse_server_response(response: &str) -> Result<Msg, Fe2IoError> {
    let msg: Msg = serde_json::from_str(response)?;
    debug!("Parsed message {:?}", msg);
    Ok(msg)
}

fn match_server_response(msg: Msg, tx: &Sender<String>) -> Result<(), Fe2IoError> {
    match msg.type_.as_str() {
        "bgm" => get_audio(msg, tx)?,
        "gameStatus" => get_status(msg, tx)?,
        _ => warn!("Server sent invalid msgType {}", msg.type_),
    };
    Ok(())
}

fn get_audio(msg: Msg, tx: &Sender<String>) -> Result<(), Fe2IoError> {
    let url = msg.audio_url
        .ok_or(Fe2IoError::Invalid("Server sent response of type bgm but no URL was provided".to_owned()))?;
    debug!("Playing audio {}", url);
    tx.send(url)?;
    Ok(())
}

fn get_status(msg: Msg, tx: &Sender<String>) -> Result<(), Fe2IoError> {
    let status_type = msg.status_type
        .ok_or(Fe2IoError::Invalid("Server sent response of type gameStatus but no status was provided".to_owned()))?;
    debug!("Set game status to {}", status_type);
    tx.send(status_type)?;
    Ok(())
}