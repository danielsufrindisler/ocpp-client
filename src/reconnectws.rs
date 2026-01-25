use futures::{SinkExt, StreamExt};
use std::backtrace::Backtrace;
use std::future::Future;
use std::io;
use std::pin::Pin;
use stream_reconnect::{ReconnectStream, UnderlyingStream};
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::handshake::client::Response;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::{error::Error as WsError, Message};
use tokio_tungstenite::{connect_async_with_config, MaybeTlsStream, WebSocketStream, tungstenite::protocol::WebSocketConfig};
use tokio_tungstenite::tungstenite::extensions::compression::deflate::DeflateConfig;


pub struct MyWs {
    protocol: String,
}

impl UnderlyingStream<Request<()>, Result<Message, WsError>, WsError> for MyWs {
    type Stream = WebSocketStream<MaybeTlsStream<TcpStream>>;

    // Establishes connection.
    // Additionally, this will be used when reconnect tries are attempted.
    fn establish(
        addr: Request<()>,
    ) -> Pin<Box<dyn Future<Output = Result<(Self::Stream, Response), WsError>> + Send>> {
        Box::pin(async move {
            // In this case, we are trying to connect to the WebSocket endpoint
            println!("Connecting to {}", addr.uri());
            //    println!("Custom backtrace: {}", Backtrace::force_capture());
            let mut config = WebSocketConfig::default();
            config.extensions.permessage_deflate = Some(DeflateConfig::default());
            let (ws_connection, response) = connect_async_with_config(addr, Some(config), false).await?;
            Ok((ws_connection, response))
        })
    }

    // The following errors are considered disconnect errors.
    fn is_write_disconnect_error(err: &WsError) -> bool {
        matches!(
            err,
            WsError::ConnectionClosed
                | WsError::AlreadyClosed
                | WsError::Io(_)
                | WsError::Tls(_)
                | WsError::Protocol(_)
        )
    }

    // If an `Err` is read, then there might be an disconnection.
    fn is_read_disconnect_error(item: &Result<Message, WsError>) -> bool {
        if let Err(e) = item {
            Self::is_write_disconnect_error(e)
        } else {
            false
        }
    }

    // Return "Exhausted" if all retry attempts are failed.
    fn exhaust_err() -> WsError {
        WsError::Io(io::Error::new(io::ErrorKind::Other, "Exhausted"))
    }
}

pub type ReconnectWs = ReconnectStream<MyWs, Request<()>, Result<Message, WsError>, WsError>;
