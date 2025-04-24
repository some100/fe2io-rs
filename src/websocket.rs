use crate::{Args, Fe2IoError};
use futures_util::SinkExt;
use tokio::{
    net::TcpStream,
    time::{sleep, Duration},
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{self, Message},
    MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error, info, warn};

pub async fn connect_to_server(
    args: &Args,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Fe2IoError> {
    let mut delay = args.delay;
    let mut retries = 1;
    let (mut server, _) = loop {
        match connect_async(&args.url).await {
            Ok(server) => break server,
            Err(tungstenite::Error::Url(e)) => return Err(Fe2IoError::WebSocket(e.into())), // if url is invalid, dont bother retrying since theres no hope of it working
            Err(e) => {
                warn!(
                    "Failed to connect to server {}, retrying in {} seconds. {}/{}",
                    args.url, delay, retries, args.attempts
                );
                debug!("Failed to connect: {e}");
                (delay, retries) = delay_reconnect(delay, retries, args).await?;
            }
        }
    };
    server.send(Message::Text((&args.username).into())).await?;
    info!(
        "Connected to server {} with username {}",
        args.url, args.username
    );
    Ok(server)
}

async fn delay_reconnect(
    mut delay: u64,
    mut retries: u64,
    args: &Args,
) -> Result<(u64, u64), Fe2IoError> {
    if retries >= args.attempts {
        error!(
            "Failed to connect to server after {} attempts, bailing",
            args.attempts
        );
        return Err(Fe2IoError::NoRetry);
    }
    sleep(Duration::from_secs(delay)).await;
    delay = (delay * args.backoff).min(args.max_delay);
    retries += 1;
    Ok((delay, retries))
}

pub async fn reconnect_to_server(
    args: &Args,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Fe2IoError> {
    warn!("Lost connection to server, attempting to reconnect");
    connect_to_server(args).await
}
