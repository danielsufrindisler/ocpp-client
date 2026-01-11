use crate::client::Client;
use crate::reconnectws::ReconnectWs;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use std::time::Duration;
use stream_reconnect::ReconnectOptions;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, SEC_WEBSOCKET_PROTOCOL};
use tokio_tungstenite::tungstenite::http::Request;
use url::Url;

#[cfg(feature = "ocpp_1_6")]
use crate::ocpp_1_6::OCPP1_6Client;

#[cfg(feature = "ocpp_2_0_1")]
use crate::ocpp_2_0_1::OCPP2_0_1Client;

/// Connect to an OCPP server using the best OCPP version available.
pub async fn connect(
    address: &str,
    options: Option<ConnectOptions<'_>>,
) -> Result<Client, Box<dyn std::error::Error + Send + Sync>> {
    let (stream, protocol) = setup_socket(address, "ocpp1.6, ocpp2.0.1", options).await?;

    match protocol.as_str() {
        #[cfg(feature = "ocpp_1_6")]
        "ocpp1.6" => Ok(Client::OCPP1_6(OCPP1_6Client::new(stream))),
        #[cfg(feature = "ocpp_2_0_1")]
        "ocpp2.0.1" => Ok(Client::OCPP2_0_1(OCPP2_0_1Client::new(stream))),
        _ => Err("The CSMS server has selected a protocol that we don't support".into()),
    }
}

/// Connect to an OCPP 1.6 server.
#[cfg(feature = "ocpp_1_6")]
pub async fn connect_1_6(
    address: &str,
    options: Option<ConnectOptions<'_>>,
) -> Result<OCPP1_6Client, Box<dyn std::error::Error + Send + Sync>> {
    let (stream, _) = setup_socket(address, "ocpp1.6", options).await?;
    Ok(OCPP1_6Client::new(stream))
}

/// Connect to an OCPP 2.0.1 server.
pub async fn connect_2_0_1(
    address: &str,
    options: Option<ConnectOptions<'_>>,
) -> Result<OCPP2_0_1Client, Box<dyn std::error::Error + Send + Sync>> {
    let (stream, _) = setup_socket(address, "ocpp2.0.1", options).await?;
    Ok(OCPP2_0_1Client::new(stream))
}

async fn setup_socket(
    address: &str,
    protocols: &str,
    options: Option<ConnectOptions<'_>>,
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
    if let Some(options) = options {
        if let Some(username) = options.username {
            let data = format!("{}:{}", username, options.password.unwrap_or(""));
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

    Ok((stream, protocol.to_str()?.to_string()))
}

#[derive(Debug, Clone)]
pub struct ConnectOptions<'a> {
    pub username: Option<&'a str>,
    pub password: Option<&'a str>,
}
