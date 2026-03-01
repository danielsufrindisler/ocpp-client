use crate::raw_ocpp_common_call::{RawOcppCommonCall, RawOcppCommonError, RawOcppCommonResult};
use crate::reconnectws::ReconnectWs;
use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt, TryStreamExt};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::Sender;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use tokio_tungstenite::tungstenite::Utf8Bytes;

/// Common base for OCPP clients - holds shared state
#[derive(Clone)]
pub struct CommonOcppClientBase {
    pub sink: Arc<Mutex<Option<SplitSink<ReconnectWs, Message>>>>,
    pub response_channels:
        Arc<Mutex<BTreeMap<Uuid, oneshot::Sender<Result<Value, RawOcppCommonError>>>>>,
    pub request_senders: Arc<Mutex<BTreeMap<String, mpsc::Sender<RawOcppCommonCall>>>>,
    pub pong_channels: Arc<Mutex<VecDeque<oneshot::Sender<()>>>>,
    pub ping_sender: Sender<()>,
    pub timeout: Duration,
}

impl CommonOcppClientBase {
    /// Create a new client base without initializing the connection
    pub fn new_unconnected() -> Self {
        let (ping_sender, _) = tokio::sync::broadcast::channel(10);

        Self {
            sink: Arc::new(Mutex::new(None)),
            response_channels: Arc::new(Mutex::new(BTreeMap::new())),
            request_senders: Arc::new(Mutex::new(BTreeMap::new())),
            pong_channels: Arc::new(Mutex::new(VecDeque::new())),
            ping_sender,
            timeout: Duration::from_secs(5),
        }
    }

    /// Initialize the connection with a stream and spawn the message handler
    pub async fn initialize_connection(&self, stream: ReconnectWs) {
        let (sink, stream) = stream.split();

        let response_channels2 = Arc::clone(&self.response_channels);
        let pong_channels2 = Arc::clone(&self.pong_channels);
        let request_senders2 = self.request_senders.clone();
        let sink2 = self.sink.clone();
        let ping_sender2 = self.ping_sender.clone();

        tokio::spawn(async move {
            stream
                .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e))
                .try_for_each(|message| {
                    let response_channels2 = response_channels2.clone();
                    let ping_sender = ping_sender2.clone();
                    let pong_channels2 = pong_channels2.clone();
                    let request_senders = request_senders2.clone();
                    let _sink = sink2.clone();
                    async move {
                        match message {
                            Message::Text(raw_payload) => {
                                let raw_value = serde_json::from_str(&raw_payload)?;

                                match raw_value {
                                    Value::Array(list) => {
                                        if let Some(message_type_item) = list.get(0) {
                                            if let Value::Number(message_type_raw) = message_type_item {
                                                if let Some(message_type) = message_type_raw.as_u64() {
                                                    match message_type {
                                                        // CALL
                                                        2 => {
                                                            let call: RawOcppCommonCall =
                                                                serde_json::from_str(&raw_payload).unwrap();
                                                            let action = &call.2;
                                                            let sender_opt = {
                                                                // Try to acquire lock, fail immediately if unavailable
                                                                match request_senders.try_lock() {
                                                                    Ok(lock) => lock.get(action).cloned(),
                                                                    Err(_) => {
                                                                        // Lock not available, will retry next iteration
                                                                        None
                                                                    }
                                                                }
                                                            };
                                                            if let Some(sender) = sender_opt {
                                                                if let Err(err) = sender.send(call).await {
                                                                    println!("Error sending request: {:?}", err);
                                                                };
                                                            }
                                                        },
                                                        // RESPONSE
                                                        3 => {
                                                            let result: RawOcppCommonResult =
                                                                serde_json::from_str(&raw_payload).unwrap();
                                                            loop {
                                                                match response_channels2.try_lock() {
                                                                    Ok(mut lock) => {
                                                                        if let Some(sender) = lock.remove(&Uuid::parse_str(&result.1)?) {
                                                                            sender.send(Ok(result.2)).unwrap();
                                                                        }
                                                                        break;
                                                                    }
                                                                    Err(_) => {
                                                                        // Lock not available, sleep and retry
                                                                        tokio::time::sleep(Duration::from_millis(1)).await;
                                                                    }
                                                                }
                                                            }
                                                        },
                                                        // ERROR
                                                        4 => {
                                                            let error: RawOcppCommonError =
                                                                serde_json::from_str(&raw_payload)?;
                                                            loop {
                                                                match response_channels2.try_lock() {
                                                                    Ok(mut lock) => {
                                                                        if let Some(sender) = lock.remove(&Uuid::parse_str(&error.1)?) {
                                                                            sender.send(Err(error.clone())).unwrap();
                                                                        }
                                                                        break;
                                                                    }
                                                                    Err(_) => {
                                                                        // Lock not available, sleep and retry
                                                                        tokio::time::sleep(Duration::from_millis(1)).await;
                                                                    }
                                                                }
                                                            }
                                                        },
                                                        _ => println!("Unknown message type"),
                                                    }
                                                } else {
                                                    println!("The message type has to be an integer, it cant have decimals")
                                                }
                                            } else {
                                                println!("The first item in the array was not a number")
                                            }
                                        } else {
                                            println!("The root list was empty")
                                        }
                                    }
                                    _ => println!("A message should be an array of items"),
                                }
                            }
                            Message::Ping(_) => {
                                if ping_sender.receiver_count() > 0 {
                                    if let Err(err) = ping_sender.send(()) {
                                        println!("Error sending websocket ping: {:?}", err);
                                    };
                                }
                            }
                            Message::Pong(_) => {
                                loop {
                                    match pong_channels2.try_lock() {
                                        Ok(mut lock) => {
                                            if let Some(sender) = lock.pop_back() {
                                                sender.send(()).unwrap();
                                            }
                                            break;
                                        }
                                        Err(_) => {
                                            // Lock not available, sleep and retry
                                            tokio::time::sleep(Duration::from_millis(1)).await;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                        Ok(())
                    }
                }).await?;
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });

        // Initialize the sink
        let mut sink_lock = self.sink.lock().await;
        *sink_lock = Some(sink);
    }

    /// Spawn a connection task that establishes the WebSocket connection
    pub fn spawn_connection_task(
        &self,
        address: &str,
        protocol: &str,
        username: Option<String>,
        password: Option<String>,
    ) {
        let base_clone = self.clone();
        let address_str = address.to_string();
        let proto_clone = protocol.to_string();

        tokio::spawn(async move {
            let options = crate::ConnectOptions {
                username: username.as_deref(),
                password: password.as_deref(),
            };
            
            if let Ok((stream, _protocol)) = crate::setup_socket(&address_str, &proto_clone, Some(options)).await {
                println!("Connection established");
                base_clone.initialize_connection(stream).await;
            }
        });
    }

    /// Initialize the common client base with shared state and message handling (old method for backwards compatibility)
    pub(crate) fn new(stream: ReconnectWs) -> Self {
        let (sink, stream) = stream.split();
        let sink = Arc::new(Mutex::new(Some(sink)));

        let response_channels = Arc::new(Mutex::new(BTreeMap::<
            Uuid,
            oneshot::Sender<Result<Value, RawOcppCommonError>>,
        >::new()));
        let response_channels2 = Arc::clone(&response_channels);

        let pong_channels = Arc::new(Mutex::new(VecDeque::<oneshot::Sender<()>>::new()));
        let pong_channels2 = Arc::clone(&pong_channels);

        let request_senders: Arc<Mutex<BTreeMap<String, mpsc::Sender<RawOcppCommonCall>>>> =
            Arc::new(Mutex::new(BTreeMap::new()));

        let request_senders2 = request_senders.clone();
        let sink2 = sink.clone();

        let (ping_sender, _) = tokio::sync::broadcast::channel(10);
        let ping_sender2 = ping_sender.clone();

        tokio::spawn(async move {
            stream
                .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e))
                .try_for_each(|message| {
                    let response_channels2 = response_channels2.clone();
                    let ping_sender = ping_sender2.clone();
                    let pong_channels2 = pong_channels2.clone();
                    let request_senders = request_senders2.clone();
                    let _sink = sink2.clone();
                    async move {
                        match message {
                            Message::Text(raw_payload) => {
                                let raw_value = serde_json::from_str(&raw_payload)?;

                                match raw_value {
                                    Value::Array(list) => {
                                        if let Some(message_type_item) = list.get(0) {
                                            if let Value::Number(message_type_raw) = message_type_item {
                                                if let Some(message_type) = message_type_raw.as_u64() {
                                                    match message_type {
                                                        // CALL
                                                        2 => {
                                                            let call: RawOcppCommonCall =
                                                                serde_json::from_str(&raw_payload).unwrap();
                                                            let action = &call.2;
                                                            let sender_opt = {
                                                                let lock = request_senders.lock().await;
                                                                lock.get(action).cloned()
                                                            };
                                                            if let Some(sender) = sender_opt {
                                                                if let Err(err) = sender.send(call).await {
                                                                    println!("Error sending request: {:?}", err);
                                                                };
                                                            }
                                                        },
                                                        // RESPONSE
                                                        3 => {
                                                            let result: RawOcppCommonResult =
                                                                serde_json::from_str(&raw_payload).unwrap();
                                                            let mut lock = response_channels2.lock().await;
                                                            if let Some(sender) = lock.remove(&Uuid::parse_str(&result.1)?) {
                                                                sender.send(Ok(result.2)).unwrap();
                                                            }
                                                        },
                                                        // ERROR
                                                        4 => {
                                                            let error: RawOcppCommonError =
                                                                serde_json::from_str(&raw_payload)?;
                                                            let mut lock = response_channels2.lock().await;
                                                            if let Some(sender) = lock.remove(&Uuid::parse_str(&error.1)?) {
                                                                sender.send(Err(error)).unwrap();
                                                            }
                                                        },
                                                        _ => println!("Unknown message type"),
                                                    }
                                                } else {
                                                    println!("The message type has to be an integer, it cant have decimals")
                                                }
                                            } else {
                                                println!("The first item in the array was not a number")
                                            }
                                        } else {
                                            println!("The root list was empty")
                                        }
                                    }
                                    _ => println!("A message should be an array of items"),
                                }
                            }
                            Message::Ping(_) => {
                                if ping_sender.receiver_count() > 0 {
                                    if let Err(err) = ping_sender.send(()) {
                                        println!("Error sending websocket ping: {:?}", err);
                                    };
                                }
                            }
                            Message::Pong(_) => {
                                let mut lock = pong_channels2.lock().await;
                                if let Some(sender) = lock.pop_back() {
                                    sender.send(()).unwrap();
                                }
                            }
                            _ => {}
                        }
                        Ok(())
                    }
                }).await?;
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });

        Self {
            sink,
            response_channels,
            request_senders,
            pong_channels,
            ping_sender,
            timeout: Duration::from_secs(5),
        }
    }

    /// Send a raw request and wait for response
    pub async fn do_send_request_raw(
        &self,
        message_id: Uuid,
        call: RawOcppCommonCall,
    ) -> Result<Result<Value, RawOcppCommonError>, Box<dyn std::error::Error + Send + Sync>> {
        {
            println!("Sending {:?}", call);
            let mut lock = self.sink.lock().await;
            if let Some(sink) = lock.as_mut() {
                sink.send(Message::Text(Utf8Bytes::from(serde_json::to_string(&call)?)))
                    .await?;
            } else {
                return Err("Sink not connected".into());
            }
        }

        let (s, r) = oneshot::channel();
        {
            let mut response_channels = self.response_channels.lock().await;
            response_channels.insert(message_id, s);
        }

        match timeout(self.timeout, r).await? {
            Ok(res) => match res {
                Ok(value) => Ok(Ok(value)),
                Err(e) => Ok(Err(e)),
            },
            Err(_) => Err("Timeout".into()),
        }
    }

    /// Send a serialized request and wait for response
    pub async fn do_send_request<P: Serialize, R: DeserializeOwned>(
        &self,
        request: P,
        action: &str,
    ) -> Result<Result<R, RawOcppCommonError>, Box<dyn std::error::Error + Send + Sync>> {
        let message_id = Uuid::new_v4();

        let call: RawOcppCommonCall = RawOcppCommonCall(
            2,
            message_id.to_string(),
            action.to_string(),
            serde_json::to_value(&request)?,
        );
        let result = self.do_send_request_raw(message_id, call).await;
        match result {
            Ok(ocpp_result) => match ocpp_result {
                Ok(value) => Ok(Ok(serde_json::from_value::<R>(value)?)),
                Err(e) => Ok(Err(e)),
            },
            Err(e) => Err(e),
        }
    }
}
