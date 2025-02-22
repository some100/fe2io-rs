use std::env;
use std::io;
use std::io::Cursor;
use std::net::TcpStream;
use rodio::{Sink, Decoder, OutputStream};
use serde_json::Value;
use tungstenite::{WebSocket, connect, Message};
use tungstenite::stream::MaybeTlsStream;
use reqwest::blocking::get;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let username = &args[1];
    let volume = if args.len() > 2 {
        args[2].parse::<f32>()?
    } else {
        0.5
    };
    let url = if args.len() > 3 {
        &args[3]
    } else {
        "ws://client.fe2.io:8081"
    };
    let mut server = connect_to_server(url, username)?;

    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;

    loop {
        handle_events(&mut server, &sink, volume)?;
    }
}

fn parse_args() -> io::Result<Vec<String>> {
    let args: Vec<String> = env::args().collect();
    if args.len() == 1 {
        eprintln!("Usage: fe2io-rs username [volume] [customurl]");
        std::process::exit(1);
    }
    Ok(args)
}

fn connect_to_server(url: &str, username: &str) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, Box<dyn std::error::Error>> {
    let (mut server, _) = connect(url)?;
    server.send(Message::Text(username.into()))?;
    println!("Connected to server {url} with username {username}");
    Ok(server)
}

fn handle_events(server: &mut WebSocket<MaybeTlsStream<TcpStream>>, sink: &Sink, volume: f32) -> Result<(), Box<dyn std::error::Error>> {
    let response = server.read()?;
    let response_as_text = response.to_text()?;
    let msg: Value = serde_json::from_str(response_as_text)?;
    let msg_type = &msg["msgType"];
    match msg_type.as_str() {
        Some("bgm") => play_audio(msg, &sink)?,
        Some("gameStatus") => handle_status(msg, &sink, volume)?,
        _ => (),
    };
    Ok(())
}

fn play_audio(msg: Value, sink: &Sink) -> Result<(), Box<dyn std::error::Error>> {
    sink.clear();
    let url = msg["audioUrl"]
        .as_str()
        .ok_or("URL {url} isn't valid")?;
    let audio = get(url)?;
    let cursor = Cursor::new(audio.bytes()?);
    let source = Decoder::new(cursor)?;
    sink.append(source);
    sink.play();
    Ok(())
}

fn handle_status(msg: Value, sink: &Sink, volume: f32) -> Result<(), Box<dyn std::error::Error>> {
    let statustype = msg["statusType"].as_str();
    match statustype {
        Some("died") => sink.set_volume(volume),
        Some("left") => sink.clear(),
        _ => (),
    }
    Ok(())
}