use std::io::Cursor;
use std::net::TcpStream;
use clap::Parser;
use rodio::{Sink, Decoder, OutputStream};
use serde::Deserialize;
use tungstenite::{WebSocket, connect, Message};
use tungstenite::stream::MaybeTlsStream;
use anyhow::{Context, anyhow, Result};
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

fn main() -> Result<()> {
    let env = env_logger::Env::default().filter_or("LOG_LEVEL", "info");
    env_logger::init_from_env(env);

    let args = Args::parse();
    let mut server = match connect_to_server(&args.url, &args.username, args.delay, args.backoff, args.attempts) {
        Ok(server) => server,
        Err(e) => {
            error!("{}", e);
            return Ok(());
        },
    };

    let (_stream, stream_handle) = match OutputStream::try_default() {
        Ok(stream) => stream,
        Err(_e) => {
            error!("Failed to open audio device, most likely because none are available");
            return Ok(());
        },
    };
    let sink = Sink::try_new(&stream_handle)?;

    loop {
        match handle_events(&mut server, &sink, &args) {
            Err(e) if e.to_string() == "Connection closed normally" => {
                warn!("Lost connection to server, attempting to reconnect");
                server = match connect_to_server(&args.url, &args.username, args.delay, args.backoff, args.attempts) {
                    Ok(server) => server,
                    Err(e) => {
                        error!("{}", e);
                        return Ok(());
                    },
                };
            },
            Err(e) => error!("Error: {}", e),
            _ => (),
        }
    }
}

fn connect_to_server(url: &str, username: &str, delay: u64, backoff: u64, attempts: u64) -> Result<WebSocket<MaybeTlsStream<TcpStream>>> {
    let mut delay = delay;
    let mut retries = 1;
    let (mut server, _) = loop {
        match connect(url) {
            Ok(server) => break server,
            Err(_e) => {
                if retries > attempts {
                    return Err(anyhow!("Attempts exhausted, exiting"));
                }
                warn!("Failed to connect to server {}, retrying in {} seconds. {}/{}", url, delay, retries, attempts);
                std::thread::sleep(std::time::Duration::from_secs(delay));
                delay *= backoff;
                retries += 1;
            }
        }
    };
    server.send(Message::Text(username.into()))
        .with_context(|| format!("Failed to connect to server {}", url))?;
    info!("Connected to server {} with username {}", url, username);
    Ok(server)
}

fn handle_events(server: &mut WebSocket<MaybeTlsStream<TcpStream>>, sink: &Sink, args: &Args) -> Result<()> {
    let response = server.read()?;
    let response_as_text = response.to_text()?;
    let msg: Msg = serde_json::from_str(response_as_text)
        .with_context(|| format!("Server sent invalid response {}", response_as_text))?;
    let msg_type = msg.msg_type.as_str();
    match msg_type {
        "bgm" => play_audio(msg, &sink)?,
        "gameStatus" => handle_status(msg, &sink, &args)?,
        _ => error!("Server sent invalid msgType {}", msg_type),
    };
    Ok(())
}

fn play_audio(msg: Msg, sink: &Sink) -> Result<()> {
    sink.stop();
    let url = msg.audio_url
        .context("Server sent response of type bgm but no URL was provided")?;
    let audio = reqwest::blocking::get(&url)
        .with_context(|| format!("URL {} is invalid", url))?;
    let cursor = Cursor::new(audio.bytes()?);
    let source = Decoder::new(cursor)?;
    sink.append(source);
    debug!("Playing audio {}", url);
    Ok(())
}

fn handle_status(msg: Msg, sink: &Sink, args: &Args) -> Result<()> {
    let statustype = msg.status_type
        .context("Server sent response of type gameStatus but no status was provided")?;
    match statustype.as_str() {
        "died" => sink.set_volume(args.volume),
        "left" => sink.clear(),
        _ => (),
    }
    debug!("Set game status to {}", statustype);
    Ok(())
}