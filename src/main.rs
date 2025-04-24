mod audio;
mod error;
mod event;
mod websocket;

use crate::error::Fe2IoError;
use clap::Parser;
use rodio::{OutputStream, Sink};
use serde::Deserialize;
use tokio::{signal::ctrl_c, sync::mpsc::channel, task::JoinSet};
use tracing::{error, warn, Level};

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
    #[serde(alias = "msg_type", alias = "msgType")]
    // fe2io compat (also clippy pedantic wouldnt stop complaining)
    type_: String,
    #[serde(alias = "audioUrl")]
    audio_url: Option<String>,
    #[serde(alias = "statusType")]
    status_type: Option<String>,
}

enum MsgValue {
    Volume(String),
    Audio(String),
}

#[tokio::main]
async fn main() -> Result<(), Fe2IoError> {
    let mut tasks = JoinSet::new();

    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let args = Args::parse();

    let server = websocket::connect_to_server(&args).await?;

    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;

    let (tx, rx) = channel(32); // there is no case where you'd need this much capacity
    tasks.spawn(audio::audio_loop(sink, rx, args.clone())); // if we borrow instead we get an &Args doesn't live for long enough error
    tasks.spawn(event::event_loop(server, tx, args));

    tokio::select! {
        res = wait_for_tasks(&mut tasks) => {
            if let Err(e) = res {
                error!("{e}");
            }
        },
        _ = ctrl_c() => {
            warn!("Received interrupt, exiting");
            tasks.shutdown().await;
        }
    }
    Ok(())
}

async fn wait_for_tasks(tasks: &mut JoinSet<Result<(), Fe2IoError>>) -> Result<(), Fe2IoError> {
    match tasks.join_next().await {
        Some(Err(e)) => return Err(Fe2IoError::Join(e)),
        None => return Err(Fe2IoError::NoTasks), // something really bad must have happened for this to be the case
        _ => warn!("At least one task exited, ending program"),
    }
    Ok(())
}
