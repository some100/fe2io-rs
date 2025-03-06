use crate::{Fe2IoError, Args};
use std::io::Cursor;
use tokio::{
    time::Duration,
    sync::mpsc::Receiver,
};
use rodio::{Sink, Decoder};
use reqwest::Client;
use log::error;

pub async fn audio_loop(sink: Sink, mut rx: Receiver<String>, args: Args) -> Result<(), Fe2IoError> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .build()?;
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
    sink.set_volume(1.0); // Volume is set to 1.0 by default. If this is too low or too high, you can manually change your volume
    sink.stop();
    let response = client
        .get(input)
        .send()
        .await?;
    let audio = match response.error_for_status() {
        Ok(audio) => audio,
        Err(e) => return Err(Fe2IoError::Http(e)),
    };
    let cursor = Cursor::new(audio.bytes().await?);
    let source = Decoder::new(cursor)?;
    sink.append(source);
    Ok(())
}