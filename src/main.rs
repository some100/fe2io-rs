use std::io::Cursor;
use std::net::TcpStream;
use std::time::Duration;
use std::thread::sleep;
use clap::Parser;
use rodio::{Sink, Decoder, OutputStream};
use serde_json::Value;
use tungstenite::{WebSocket, connect, Message};
use tungstenite::stream::MaybeTlsStream;
use reqwest::blocking::get;
use anyhow::{Context, anyhow, Result};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    username: String,
    #[arg(short, long, default_value_t = 0.5)]
    volume: f32,
    #[arg(short, long, default_value = "ws://client.fe2.io:8081")]
    url: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut server = connect_to_server(&args.url, &args.username)?;

    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;

    loop {
        match handle_events(&mut server, &sink, &args) {
            Err(e) if e.to_string() == "Connection closed normally" => {
                eprintln!("Lost connection to server, attempting to reconnect");
                server = connect_to_server(&args.url, &args.username)?;
            },
            Err(e) => eprintln!("Error: {e}"),
            _ => (),
        }
    }
}

fn connect_to_server(url: &str, username: &str) -> Result<WebSocket<MaybeTlsStream<TcpStream>>> {
    let mut delay = 1;
    let (mut server, _) = loop {
        if let Ok(server) = connect(url) {
            break server;
        }
        eprintln!("Failed to connect to server {url}, retrying in {delay} seconds");
        sleep(Duration::from_secs(delay));
        delay *= 2;
    };
    server.send(Message::Text(username.into()))
        .with_context(|| format!("Failed to connect to server {}", url))?;
    println!("Connected to server {url} with username {username}");
    Ok(server)
}

fn handle_events(server: &mut WebSocket<MaybeTlsStream<TcpStream>>, sink: &Sink, args: &Args) -> Result<()> {
    let response = server.read()?;
    let response_as_text = response.to_text()?;
    let msg: Value = serde_json::from_str(response_as_text)
        .with_context(|| format!("Server sent back invalid response {}", response_as_text))?;
    let msg_type = &msg["msgType"].as_str()
        .ok_or_else(|| anyhow!("Server sent back response without msgType"))?;
    match *msg_type {
        "bgm" => play_audio(msg, &sink)?,
        "gameStatus" => handle_status(msg, &sink, &args)?,
        _ => (),
    };
    Ok(())
}

fn play_audio(msg: Value, sink: &Sink) -> Result<()> {
    sink.clear();
    let url = msg["audioUrl"]
        .as_str()
        .ok_or_else(|| anyhow!("Server sent response of type bgm but no URL was provided"))?;
    let audio = get(url)
        .with_context(|| format!("URL {} is invalid", url))?;
    let cursor = Cursor::new(audio.bytes()?);
    let source = Decoder::new(cursor)?;
    sink.append(source);
    sink.play();
    println!("Playing audio {url}");
    Ok(())
}

fn handle_status(msg: Value, sink: &Sink, args: &Args) -> Result<()> {
    let statustype = msg["statusType"]
        .as_str()
        .ok_or_else(|| anyhow!("Server sent response of type gameStatus but no status was provided"))?;
    match statustype {
        "died" => sink.set_volume(args.volume),
        "left" => sink.clear(),
        _ => (),
    }
    println!("Set game status to {statustype}");
    Ok(())
}