use crate::{Args, Fe2IoError, MsgValue};
use log::error;
use reqwest::Client;
use rodio::{Decoder, Sink};
use std::io::Cursor;
use tokio::{sync::mpsc::Receiver, time::Duration};

pub async fn audio_loop(
    sink: Sink,
    mut rx: Receiver<MsgValue>,
    args: Args,
) -> Result<(), Fe2IoError> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .build()?;
    loop {
        match handle_audio_inputs(&sink, &mut rx, &client, &args).await {
            Err(Fe2IoError::RecvClosed()) => {
                error!("Audio receiver channels closed");
                return Err(Fe2IoError::RecvClosed()); // this is not a recoverable error, so just return from loop
            }
            Err(e) => error!("{e}"),
            _ => (),
        }
    }
}

async fn handle_audio_inputs(
    sink: &Sink,
    rx: &mut Receiver<MsgValue>,
    client: &Client,
    args: &Args,
) -> Result<(), Fe2IoError> {
    let input = rx.recv().await.ok_or(Fe2IoError::RecvClosed())?;
    match input {
        MsgValue::Volume(input) => change_status(sink, &input, args)?,
        MsgValue::Audio(input) => play_audio(sink, client, &input).await?,
    }
    Ok(())
}

fn change_status(sink: &Sink, input: &str, args: &Args) -> Result<(), Fe2IoError> {
    match input {
        "died" => sink.set_volume(args.volume),
        "left" => sink.stop(),
        _ => return Err(Fe2IoError::Invalid(format!("Got invalid status type {input}"))),
    }
    Ok(())
}

async fn play_audio(sink: &Sink, client: &Client, input: &str) -> Result<(), Fe2IoError> {
    sink.set_volume(1.0); // Volume is set to 1.0 by default. If this is too low or too high, you can manually change your volume
    sink.stop();
    let response = client.get(input).send().await?;
    let audio = match response.error_for_status() {
        Ok(audio) => audio,
        Err(e) => return Err(Fe2IoError::Http(e)),
    };
    let cursor = Cursor::new(audio.bytes().await?);
    let source = Decoder::new(cursor)?;
    sink.append(source);
    Ok(())
}
