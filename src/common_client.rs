use crate::raw_ocpp_common_call::{RawOcppCommonCall, RawOcppCommonError, RawOcppCommonResult};
use crate::reconnectws::ReconnectWs;
use crate::logger::CP_ID;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt, TryStreamExt};
use log::{debug, error, info, warn};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::Sender;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::Utf8Bytes;
use uuid::Uuid;
/// Common base for OCPP clients - holds shared state
#[derive(Clone)]

pub struct CommonOcppClientBase {
    pub sink: Arc<Mutex<Option<SplitSink<ReconnectWs, Message>>>>,
    pub stream: Arc<Mutex<Option<SplitStream<ReconnectWs>>>>,
    pub response_channels:
        Arc<Mutex<BTreeMap<Uuid, oneshot::Sender<Result<Value, RawOcppCommonError>>>>>,
    pub request_senders: Arc<Mutex<BTreeMap<String, mpsc::Sender<RawOcppCommonCall>>>>,
    pub pong_channels: Arc<Mutex<VecDeque<oneshot::Sender<()>>>>,
    pub ping_sender: Sender<()>,
    pub timeout: Duration,
    pub username: Option<String>,
    pub password: Option<String>,
    pub address_str: String,
    pub proto_str: String,
    pub handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub cp_id: Option<usize>,
}

impl CommonOcppClientBase {
    /// Create a new client base without initializing the connection
    pub fn new_unconnected(
        username: Option<String>,
        password: Option<String>,
        address_str: String,
        proto_str: String,
    ) -> Self {
        let (ping_sender, _) = tokio::sync::broadcast::channel(10);

        // Try to get CP_ID from task local context
        let cp_id = CP_ID.try_with(|id| *id).ok();

        let mut self_ = Self {
            sink: Arc::new(Mutex::new(None)),
            stream: Arc::new(Mutex::new(None)),
            response_channels: Arc::new(Mutex::new(BTreeMap::new())),
            request_senders: Arc::new(Mutex::new(BTreeMap::new())),
            pong_channels: Arc::new(Mutex::new(VecDeque::new())),
            ping_sender,
            timeout: Duration::from_secs(5),
            username,
            password,
            address_str,
            proto_str,
            handle: Arc::new(Mutex::new(None)),
            cp_id,
        };

        self_.establish_connection();
        self_
    }

    pub fn establish_connection(&mut self) {
        // This will be called by the client after creating the connection, to set the sink and stream
        let sink2 = self.sink.clone();
        let stream3 = self.stream.clone();
        let response_channels2 = Arc::clone(&self.response_channels);
        let pong_channels2 = Arc::clone(&self.pong_channels);
        let request_senders2 = self.request_senders.clone();
        let ping_sender2 = self.ping_sender.clone();

        let stream_lock = self.stream.clone();
        let sink_lock = self.sink.clone();
        let username = self.username.clone();
        let password = self.password.clone();
        let address_str = self.address_str.clone();
        let proto_str = self.proto_str.clone();
        let handle_holder = self.handle.clone();
        let cp_id = self.cp_id.unwrap();
        
        let handle = tokio::spawn(async move {
           
            CP_ID.scope(cp_id, async {
                Self::establish_connection_inner(
                    sink2, stream3, response_channels2, pong_channels2,
                    request_senders2, ping_sender2, stream_lock, sink_lock,
                    username, password, address_str, proto_str, handle_holder,
                ).await
            }).await
            
        });

        debug!("handle is for connection task {}", handle.id());
        self.handle
            .clone()
            .try_lock()
            .expect("THIS SHOULD NOT HAPPEN")
            .replace(handle);
    }

    async fn establish_connection_inner(
        sink2: Arc<Mutex<Option<SplitSink<ReconnectWs, Message>>>>,
        stream3: Arc<Mutex<Option<SplitStream<ReconnectWs>>>>,
        response_channels2: Arc<Mutex<BTreeMap<Uuid, oneshot::Sender<Result<Value, RawOcppCommonError>>>>>,
        pong_channels2: Arc<Mutex<VecDeque<oneshot::Sender<()>>>>,
        request_senders2: Arc<Mutex<BTreeMap<String, mpsc::Sender<RawOcppCommonCall>>>>,
        ping_sender2: Sender<()>,
        stream_lock: Arc<Mutex<Option<SplitStream<ReconnectWs>>>>,
        sink_lock: Arc<Mutex<Option<SplitSink<ReconnectWs, Message>>>>,
        username: Option<String>,
        password: Option<String>,
        address_str: String,
        proto_str: String,
        handle_holder: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    ) {
        let options: crate::ConnectOptions = crate::ConnectOptions {
            username: username.clone(),
            password: password.clone(),
        };

        if let Ok((stream, _protocol)) =
            crate::setup_socket(&address_str, &proto_str, Some(options)).await
        {
            let (sink, stream) = stream.split();

            stream_lock.lock().await.replace(stream);
            sink_lock.lock().await.replace(sink);

            let cp_id: Option<usize> = CP_ID.try_with(|id| *id).ok();
            let handle = tokio::spawn(async move {
                info!("Starting message loop");
                let id = cp_id.unwrap();

               
                info!("Starting message loop with CP_ID: {}", id);
                CP_ID.scope(id, async {
                    Self::message_loop(
                        sink2, stream3, response_channels2, pong_channels2,
                        request_senders2, ping_sender2,
                    ).await
                }).await;             
            });
            debug!("handle is for inner receive loop {}", handle.id());
            handle_holder.lock().await.replace(handle);
        }
    }

    async fn message_loop(
        sink2: Arc<Mutex<Option<SplitSink<ReconnectWs, Message>>>>,
        stream3: Arc<Mutex<Option<SplitStream<ReconnectWs>>>>,
        response_channels2: Arc<Mutex<BTreeMap<Uuid, oneshot::Sender<Result<Value, RawOcppCommonError>>>>>,
        pong_channels2: Arc<Mutex<VecDeque<oneshot::Sender<()>>>>,
        request_senders2: Arc<Mutex<BTreeMap<String, mpsc::Sender<RawOcppCommonCall>>>>,
        ping_sender2: Sender<()>,
    ) {
        loop {
            let cp_id = CP_ID.try_with(|id| *id).ok().unwrap();

            info!("cp {}: Connected! Waiting for messages...", cp_id);

            match stream3.try_lock() {

                Ok(mut lock) => {

                    if let Some(stream2) = lock.as_mut() {

                        let _ = stream2.map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e))
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

                            info!("** Received: {}", raw_payload);

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
                                                                    error!("Lock not available for request_senders, cannot handle incoming call for action: {}", action);
                                                                    // Lock not available, will retry next iteration
                                                                    None
                                                                }
                                                            }
                                                        };
                                                        if let Some(sender) = sender_opt {
                                                            if let Err(err) = sender.send(call).await {
                                                                error!("Error sending request: {:?}", err);
                                                            };
                                                        } else {
                                                            warn!("No handler for action: {}", action);
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
                                                    _ => warn!("Bad Incoming: Unknown message type"),
                                                }
                                            } else {
                                                warn!("Bad Incoming: The message type has to be an integer, it cant have decimals")
                                            }
                                        } else {
                                            warn!("Bad Incoming: The first item in the array was not a number")
                                        }
                                    } else {
                                        warn!("Bad Incoming: The root list was empty")
                                    }
                                }
                                _ => warn!("Bad Incoming: A message should be an array of items"),
                            }
                        }
                        Message::Ping(_) => {
                            info!("Received Ping");
                            if ping_sender.receiver_count() > 0 {
                                if let Err(err) = ping_sender.send(()) {
                                    error!("Error sending websocket ping: {:?}", err);
                                };
                            }
                        }
                        Message::Pong(_) => {
                            info!("Received Pong");
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
            }).await;
                    } else {
                        // Stream not connected, sleep and retry
                        error!("Stream not connected, retrying...");
                        tokio::time::sleep(Duration::from_millis(1000)).await;
                    }
                }
                Err(_) => {
                    // Lock not available, sleep and retry
                    error!(" Lock not available, sleep and retry");
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                }
            }
        }
    }

    /// Send a raw request and wait for response
    pub async fn do_send_request_raw(
        &self,
        message_id: Uuid,
        call: RawOcppCommonCall,
    ) -> Result<Result<Value, RawOcppCommonError>, Box<dyn std::error::Error + Send + Sync>> {
        {
            debug!("Trying to send {:?}", call);
            let mut lock = self.sink.lock().await;
            if let Some(sink) = lock.as_mut() {
                let as_json = json!(call);

                let message = Utf8Bytes::from(serde_json::to_string(&as_json)?);

                let message = Message::Text(message);
                info!("{} ** SENDING: {}", self.address_str, message);

                sink.send(message).await?;
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
