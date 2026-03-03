use crate::client::Client;
use crate::reconnectws::ReconnectWs;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use log::{debug, error, info, log_enabled, Level};
use std::time::Duration;
use stream_reconnect::ReconnectOptions;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{
    HeaderValue, AUTHORIZATION, SEC_WEBSOCKET_PROTOCOL, SEC_WEBSOCKET_VERSION,
};
use tokio_tungstenite::tungstenite::http::Request;
use url::Url;

#[cfg(feature = "ocpp_1_6")]
use crate::ocpp_1_6::OCPP1_6Client;

#[cfg(feature = "ocpp_2_0_1")]
use crate::ocpp_2_0_1::OCPP2_0_1Client;

pub async fn setup_socket(
    address: &str,
    protocols: &str,
    options: Option<ConnectOptions>,
) -> Result<(ReconnectWs, String), Box<dyn std::error::Error + Send + Sync>> {
    let address = Url::parse(address)?;

    let (tx, mut rx) = mpsc::channel(100);
    let tx = std::sync::Arc::new(tokio::sync::Mutex::new(tx));

    let tx_clone = tx.clone();
    let callback = move || {
        let tx = tx_clone.clone();
        tokio::spawn(async move {
            let _ = tx.lock().await.send(()).await;
        });
    };

    let reconnect_options: ReconnectOptions = ReconnectOptions::new()
        .with_exit_if_first_connect_fails(false)
        .with_on_connect_callback(callback)
        .with_on_exhausted_callback(|| {
            // exhausted callback, we can leave it empty for now
        })
        .with_retries_generator(|| {
            vec![
                Duration::from_secs(30),
                Duration::from_secs(30),
                Duration::from_secs(30),
            ]
        });

    let mut request: Request<()> = address.to_string().into_client_request()?;

    request
        .headers_mut()
        .insert(SEC_WEBSOCKET_PROTOCOL, protocols.parse()?);

    request
        .headers_mut()
        .insert(SEC_WEBSOCKET_VERSION, HeaderValue::from_static("13"));

    if let Some(options) = options {
        if let Some(username) = options.username {
            let data = format!(
                "{}:{}",
                username,
                options.password.unwrap_or("".to_string())
            );
            let encoded = BASE64_STANDARD.encode(data);
            request
                .headers_mut()
                .insert(AUTHORIZATION, format!("Basic {}", encoded).parse()?);
        }
    }

    let stream: ReconnectWs = ReconnectWs::connect_with_options(request, reconnect_options).await?;
    rx.recv().await;

    let headers = stream.get_response().headers().clone();
    let protocol = headers
        .get(SEC_WEBSOCKET_PROTOCOL)
        .ok_or("No OCPP protocol negotiated")?;

    debug!("{:?}", headers);

    Ok((stream, protocol.to_str()?.to_string()))
}

#[derive(Debug, Clone)]
pub struct ConnectOptions {
    pub username: Option<String>,
    pub password: Option<String>,
}
