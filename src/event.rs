use crate::{websocket, Args, Fe2IoError, Msg, MsgValue};
use futures_util::StreamExt;
use log::{debug, error, warn};
use tokio::{net::TcpStream, sync::mpsc::Sender};
use tokio_tungstenite::{tungstenite, MaybeTlsStream, WebSocketStream};

pub async fn event_loop(
    mut server: WebSocketStream<MaybeTlsStream<TcpStream>>,
    tx: Sender<MsgValue>,
    args: Args,
) -> Result<(), Fe2IoError> {
    loop {
        match handle_events(&mut server, &tx).await {
            Err(Fe2IoError::Reconnect(e)) => server = {
                error!("{e}");
                websocket::reconnect_to_server(&args).await?
            },
            Err(Fe2IoError::Send(e)) => return Err(Fe2IoError::Send(e)),
            Err(e) => error!("{e}"),
            _ => (),
        }
    }
}

async fn handle_events(
    server: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    tx: &Sender<MsgValue>,
) -> Result<(), Fe2IoError> {
    let response = read_server_response(server).await?;
    let msg = parse_server_response(&response)?;
    match_server_response(msg, tx).await?;
    Ok(())
}

async fn read_server_response(
    server: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Result<String, Fe2IoError> {
    let response = server
        .next()
        .await
        .ok_or(Fe2IoError::Reconnect(tungstenite::Error::ConnectionClosed))?
        .map_err(Fe2IoError::Reconnect)?;
    debug!("Received message {response}");
    Ok(response.to_text()?.to_owned())
}

fn parse_server_response(response: &str) -> Result<Msg, Fe2IoError> {
    let msg: Msg = serde_json::from_str(response)?;
    debug!("Parsed message {:?}", msg);
    Ok(msg)
}

async fn match_server_response(msg: Msg, tx: &Sender<MsgValue>) -> Result<(), Fe2IoError> {
    match msg.type_.as_str() {
        "bgm" => get_audio(msg, tx).await?,
        "gameStatus" => get_status(msg, tx).await?,
        _ => warn!("Server sent invalid msgType {}", msg.type_),
    };
    Ok(())
}

async fn get_audio(msg: Msg, tx: &Sender<MsgValue>) -> Result<(), Fe2IoError> {
    let url = msg.audio_url.ok_or(Fe2IoError::Invalid(
        "Server sent response of type bgm but no URL was provided".to_owned(),
    ))?;
    debug!("Playing audio {url}");
    tx.send(MsgValue::Audio(url)).await?;
    Ok(())
}

async fn get_status(msg: Msg, tx: &Sender<MsgValue>) -> Result<(), Fe2IoError> {
    let status_type = msg.status_type.ok_or(Fe2IoError::Invalid(
        "Server sent response of type gameStatus but no status was provided".to_owned(),
    ))?;
    debug!("Set game status to {status_type}");
    tx.send(MsgValue::Volume(status_type)).await?;
    Ok(())
}
