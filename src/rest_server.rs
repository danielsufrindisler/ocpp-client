use crate::cp::CP;
use crate::cp_data::{
    AuthorizationType, CPData, ChargeSessionReference, EventInjectionMessage, EventTypes,
    ScheduledEvents, EV, RFID,
};
use crate::logger::CP_ID;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Deserialize)]
pub struct CreateCPRequest {
    pub name: String,
    #[serde(default)]
    pub use_original: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AddEventRequest {
    pub duration: u32,
    pub event_type: String,
    #[serde(default)]
    pub id_tag: Option<String>,
    #[serde(default)]
    pub evse_index: Option<u32>,
    #[serde(default)]
    pub connector_id: Option<u32>,
    #[serde(default)]
    pub power_vs_soc: Option<Vec<(f32, f32)>>,
    #[serde(default)]
    pub soc: Option<f32>,
    #[serde(default)]
    pub final_soc: Option<f32>,
    #[serde(default)]
    pub capacity: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AddMultipleEventsRequest {
    pub events: Vec<AddEventRequest>,
}

#[derive(Debug, Serialize)]
pub struct CreateCPResponse {
    pub cp_id: usize,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub details: Option<String>,
}

pub struct CPStore {
    pub cps: tokio::sync::RwLock<HashMap<usize, Arc<tokio::sync::Mutex<CPData>>>>,
    pub cp_instances: tokio::sync::RwLock<HashMap<usize, Arc<tokio::sync::Mutex<CP>>>>,
    pub cp_tasks: tokio::sync::RwLock<
        HashMap<
            usize,
            tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>,
        >,
    >,
    pub event_channels: tokio::sync::RwLock<HashMap<usize, mpsc::Sender<EventInjectionMessage>>>,
    pub next_id: AtomicUsize,
}

impl CPStore {
    pub fn new() -> Self {
        Self {
            cps: tokio::sync::RwLock::new(HashMap::new()),
            cp_instances: tokio::sync::RwLock::new(HashMap::new()),
            cp_tasks: tokio::sync::RwLock::new(HashMap::new()),
            event_channels: tokio::sync::RwLock::new(HashMap::new()),
            next_id: AtomicUsize::new(0),
        }
    }

    pub async fn create_cp(&self, request: CreateCPRequest) -> usize {
        let cp_id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let cp_file_name = request.name.clone();
        let use_original = request.use_original.unwrap_or(false);

        let (cp_config, variables) = CP::prepare_cp_variables_files(&cp_file_name, use_original)
            .expect("Failed to prepare CP variables files");

        CP::list_components(variables.clone());

        let cp_info = cp_config.cp.unwrap_or(crate::cp_data::CPFileInfo {
            username: None,
            password: None,
            vendor: None,
            model: None,
            serial: None,
            protocol: None,
            base_url: None,
        });

        let cp_data = CPData {
            username: cp_info.username.clone(),
            password: cp_info.password.clone(),
            supported_protocol_versions: Some("ocpp1.6, ocpp2.0.1".to_string()),
            selected_protocol: cp_info.protocol.or(Some("ocpp1.6".to_string())),
            serial: cp_info.serial.unwrap_or_else(|| format!("cp_{}", cp_id)),
            model: cp_info.model.unwrap_or_else(|| "unknown".to_string()),
            vendor: cp_info.vendor.unwrap_or_else(|| "unknown".to_string()),
            booted: false,
            evses: HashMap::new(),
            events: cp_config.events.clone(),
            authorization: None,
            variables,
            base_url: cp_info.base_url.unwrap_or_else(|| "".to_string()),
            variables_file_name: cp_file_name,
            use_original,
        };

        let cp_data_arc = Arc::new(tokio::sync::Mutex::new(cp_data));
        let mut cps = self.cps.write().await;
        cps.insert(cp_id, cp_data_arc.clone());
        drop(cps);

        // Create event injection channel
        let (event_tx, event_rx) = mpsc::channel::<EventInjectionMessage>(100);
        let mut event_channels = self.event_channels.write().await;
        event_channels.insert(cp_id, event_tx);
        drop(event_channels);

        // Create CP instance and spawn the run task
        let mut cp = CP {
            communicator: Arc::new(tokio::sync::Mutex::new(None)),
            client: None,
            data: cp_data_arc,
            event_rx: Some(event_rx),
        };

        let task = tokio::spawn(async move {
            CP_ID
                .scope(cp_id, async {
                    info!(
                        "Starting CP {} with serial {} now",
                        cp_id,
                        cp.data.lock().await.serial
                    );
                    cp.run().await
                })
                .await
        });
        info!("here");

        let mut cp_tasks = self.cp_tasks.write().await;
        cp_tasks.insert(cp_id, task);

        cp_id
    }

    pub async fn get_cp(&self, cp_id: usize) -> Option<CPData> {
        let cps = self.cps.read().await;
        if let Some(cp_arc) = cps.get(&cp_id) {
            let cp_lock = cp_arc.lock().await;
            Some(cp_lock.clone())
        } else {
            None
        }
    }

    pub async fn list_cps(&self) -> Vec<CPData> {
        let cps = self.cps.read().await;
        let mut result = Vec::new();
        for cp_arc in cps.values() {
            let cp = cp_arc.lock().await;
            result.push(cp.clone());
        }
        result
    }

    pub async fn add_event(
        &self,
        cp_id: usize,
        duration: u32,
        event_type: &str,
        id_tag: Option<String>,
        evse_index: Option<u32>,
        connector_id: Option<u32>,
        power_vs_soc: Option<Vec<(f32, f32)>>,
        soc: Option<f32>,
        final_soc: Option<f32>,
        capacity: Option<f32>,
    ) -> Result<(), String> {
        let cps = self.cps.read().await;
        if let Some(cp_arc) = cps.get(&cp_id) {
            let mut cp = cp_arc.lock().await;

            let event = match event_type {
                "authorize" => {
                    if let Some(tag) = id_tag {
                        EventTypes::Authorize(AuthorizationType::RFID(RFID { id_tag: tag }))
                    } else {
                        return Err("authorize event requires id_tag".to_string());
                    }
                }
                "communication_start" => EventTypes::CommunicationStart,
                "communication_stop" => EventTypes::CommunicationStop,
                "local_stop" => EventTypes::LocalStop,
                "plug" => {
                    if let (Some(evse_idx), Some(conn_id)) = (evse_index, connector_id) {
                        let ev = EV {
                            power_vs_soc: power_vs_soc.unwrap_or_default(),
                            soc: soc.unwrap_or(0.0),
                            final_soc: final_soc.unwrap_or(100.0),
                            capacity: capacity.unwrap_or(50.0),
                            power: 0.0,
                        };
                        EventTypes::Plug(
                            ChargeSessionReference {
                                evse_index: evse_idx,
                                connector_id: conn_id,
                            },
                            ev,
                        )
                    } else {
                        return Err("plug event requires evse_index and connector_id".to_string());
                    }
                }
                "unplug" => {
                    if let (Some(evse_idx), Some(conn_id)) = (evse_index, connector_id) {
                        EventTypes::Unplug(ChargeSessionReference {
                            evse_index: evse_idx,
                            connector_id: conn_id,
                        })
                    } else {
                        return Err("unplug event requires evse_index and connector_id".to_string());
                    }
                }
                _ => return Err(format!("unknown event type: {}", event_type)),
            };

            // Add to events vec for persistence
            cp.events.push(ScheduledEvents {
                duration,
                event: event.clone(),
            });

            // Try to inject event into running loop via channel
            let event_channels = self.event_channels.read().await;
            if let Some(tx) = event_channels.get(&cp_id) {
                if let Err(e) = tx
                    .send(EventInjectionMessage::InsertEvent {
                        event: event.clone(),
                        duration_secs: duration,
                    })
                    .await
                {
                    log::warn!("Failed to inject event into CP {} event loop: {}", cp_id, e);
                }
            }
            drop(event_channels);

            // persist events + metadata to CP JSON
            let config = crate::cp_data::CPConfigFile {
                cp: Some(crate::cp_data::CPFileInfo {
                    username: cp.username.clone(),
                    password: cp.password.clone(),
                    vendor: Some(cp.vendor.clone()),
                    model: Some(cp.model.clone()),
                    serial: Some(cp.serial.clone()),
                    protocol: cp.selected_protocol.clone(),
                    base_url: Some(cp.base_url.clone()),
                }),
                events: cp.events.clone(),
                components: cp.variables.components.clone(),
            };

            if let Err(e) =
                CP::save_cp_config_file(&format!("./data/{}.json", cp.variables_file_name), &config)
            {
                log::warn!(
                    "Failed to persist CP configuration after event update: {:?}",
                    e
                );
            }

            Ok(())
        } else {
            Err(format!("CP with ID {} not found", cp_id))
        }
    }
}

async fn create_cp_handler(
    State(store): State<Arc<CPStore>>,
    Json(payload): Json<CreateCPRequest>,
) -> impl IntoResponse {
    let cp_id = store.create_cp(payload).await;
    (
        StatusCode::CREATED,
        Json(CreateCPResponse {
            cp_id,
            message: format!("CP {} created successfully", cp_id),
        }),
    )
}

async fn get_cp_data_handler(
    State(store): State<Arc<CPStore>>,
    Path(cp_id): Path<usize>,
) -> impl IntoResponse {
    match store.get_cp(cp_id).await {
        Some(cp) => Json(serde_json::json!({
            "cp_id": cp_id,
            "username": cp.username,
            "password": cp.password,
            "vendor": cp.vendor,
            "model": cp.model,
            "serial": cp.serial,
            "protocol": cp.selected_protocol,
            "booted": cp.booted,
            "events_count": cp.events.len(),
        }))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "CP not found".to_string(),
                details: Some(format!("No CP with ID {}", cp_id)),
            }),
        )
            .into_response(),
    }
}

async fn list_cps_handler(State(store): State<Arc<CPStore>>) -> impl IntoResponse {
    let cps = store.list_cps().await;
    let cp_list: Vec<_> = cps
        .iter()
        .map(|cp| {
            serde_json::json!({
                "username": &cp.username,
                "password": &cp.password,
                "vendor": &cp.vendor,
                "model": &cp.model,
                "serial": &cp.serial,
                "protocol": &cp.selected_protocol,
                "booted": cp.booted,
                "events_count": cp.events.len(),
            })
        })
        .collect();
    Json(cp_list).into_response()
}

async fn add_event_handler(
    State(store): State<Arc<CPStore>>,
    Path(cp_id): Path<usize>,
    Json(payload): Json<AddEventRequest>,
) -> impl IntoResponse {
    match store
        .add_event(
            cp_id,
            payload.duration,
            &payload.event_type,
            payload.id_tag,
            payload.evse_index,
            payload.connector_id,
            payload.power_vs_soc,
            payload.soc,
            payload.final_soc,
            payload.capacity,
        )
        .await
    {
        Ok(_) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "message": "Event added successfully",
                "cp_id": cp_id
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Failed to add event".to_string(),
                details: Some(e),
            }),
        )
            .into_response(),
    }
}

async fn add_multiple_events_handler(
    State(store): State<Arc<CPStore>>,
    Path(cp_id): Path<usize>,
    Json(payload): Json<AddMultipleEventsRequest>,
) -> impl IntoResponse {
    let mut errors = Vec::new();
    let mut count = 0;

    for event in payload.events {
        match store
            .add_event(
                cp_id,
                event.duration,
                &event.event_type,
                event.id_tag,
                event.evse_index,
                event.connector_id,
                event.power_vs_soc,
                event.soc,
                event.final_soc,
                event.capacity,
            )
            .await
        {
            Ok(_) => count += 1,
            Err(e) => errors.push(e),
        }
    }

    if errors.is_empty() {
        (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "message": format!("{} events added successfully", count),
                "cp_id": cp_id,
                "events_added": count
            })),
        )
            .into_response()
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Failed to add some events".to_string(),
                details: Some(format!("Added {} events, errors: {:?}", count, errors)),
            }),
        )
            .into_response()
    }
}

pub async fn start_rest_server() -> tokio::task::JoinHandle<Result<(), std::io::Error>> {
    tokio::spawn(async {
        let store = Arc::new(CPStore::new());
        let app = Router::new()
            .route("/cp", post(create_cp_handler).get(list_cps_handler))
            .route("/cp/:cp_id/data", get(get_cp_data_handler))
            .route("/cp/:cp_id/events", post(add_event_handler))
            .route("/cp/:cp_id/events/bulk", post(add_multiple_events_handler))
            .with_state(store);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
            .await
            .expect("Failed to bind REST API to port 3000");

        println!("REST API Server running on http://127.0.0.1:3000");
        println!("Try: curl -X POST http://localhost:3000/cp -H 'Content-Type: application/json' -d '{{\"name\":\"0\",\"use_original\":false}}'\n");

        axum::serve(listener, app).await
    })
}
