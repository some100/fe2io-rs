use crate::{Args, Fe2IoError, MsgValue};
use log::error;
use reqwest::Client;
use rodio::{Decoder, Sink, Source};
use std::io::Cursor;
use tokio::{
    sync::mpsc::Receiver,
    time::{sleep, Duration, Instant},
};

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
            Err(Fe2IoError::RecvClosed) => return Err(Fe2IoError::RecvClosed), // this is not a continuable error, so just return from loop
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
    let input = rx.recv().await.ok_or(Fe2IoError::RecvClosed)?;
    match input {
        MsgValue::Volume(input) => change_status(sink, &input, args)?,
        MsgValue::Audio(input) => play_audio(sink, &input, client).await?,
    }
    Ok(())
}

fn change_status(sink: &Sink, input: &str, args: &Args) -> Result<(), Fe2IoError> {
    match input {
        "died" => sink.set_volume(args.volume),
        "left" => sink.stop(),
        input => {
            return Err(Fe2IoError::Invalid(format!(
                "Got invalid status type {input}"
            )))
        }
    }
    Ok(())
}

async fn play_audio(sink: &Sink, input: &str, client: &Client) -> Result<(), Fe2IoError> {
    let start = Instant::now();
    sink.set_volume(1.0); // Volume is set to 1.0 by default. If this is too low or too high, you can manually change your volume
    sink.stop();
    let response = client.get(input).send().await?;
    let audio = response.error_for_status()?;
    let cursor = Cursor::new(audio.bytes().await?);
    let source = Decoder::new(cursor)?;
    let elapsed = Instant::now().duration_since(start);
    sleep(Duration::from_millis(500)).await; // the current implementation of FE2.io is written in JavaScript. while this has worked fine for some time, it does come with inevitable varying delay. most commonly, the audio is often delayed for around 500 ms. this simulates that
    sink.append(source.skip_duration(elapsed));
    Ok(())
}
