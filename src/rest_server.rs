use crate::cp::CP;
use crate::cp_data::{
    AuthorizationType, CPData, ChargeSessionReference, EventInjectionMessage, EventTypes,
    ScheduledEvents, EV, RFID,
};
use crate::logger::CP_ID;
use crate::message_stats::MessageTracker;
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
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[derive(OpenApi)]
#[openapi(
    paths(
        create_cp_from_file_handler,
        create_cp_from_rest_handler,
        create_multiple_cps_handler,
        list_cps_handler,
        get_cp_data_handler,
        get_cp_configuration_handler,
        update_cp_configuration_handler,
        add_event_handler,
        add_multiple_events_handler,
        add_events_to_multiple_cps_handler,
        get_summary_handler
    ),
    components(
        schemas(
            CreateCPFromFileRequest,
            CreateCPFromRESTRequest,
            UpdateCPConfigurationRequest,
            CPDataResponse,
            CPConfigurationResponse,
            AddEventRequest,
            AddMultipleEventsRequest,
            AddEventsToMultipleCPsRequest,
            CreateCPResponse,
            CreateMultipleCPsResponse,
            ErrorResponse,
            crate::message_stats::TestSummary,
            crate::message_stats::CPSummary,
            crate::message_stats::MessageTypeStats,
            EventType,
            AuthorizeEventData,
            PlugEventData,
            UnplugEventData,
            FaultEventData
        )
    ),
    tags(
        (name = "Charge Points", description = "Charge Point management and configuration endpoints"),
        (name = "Events", description = "Event scheduling and injection endpoints"),
        (name = "Statistics", description = "Message statistics and monitoring endpoints")
    )
)]
struct ApiDoc;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[schema(example = json!({
    "name": "example_1",
    "use_original": false,
    "data_directory": "./data"
}))]
pub struct CreateCPFromFileRequest {
    /// Name of the charge point configuration file (without .json extension).
    /// Must have both {name}.orig.json and {name}.json files in the data directory.
    /// Example: "example_1" will load "./data/example_1.orig.json" and "./data/example_1.json"
    #[schema(example = "example_1")]
    pub name: String,

    /// Whether to use the original configuration file instead of the active one.
    /// Defaults to false, using the active {name}.json file.
    #[serde(default)]
    #[schema(example = false)]
    pub use_original: Option<bool>,

    /// Directory path containing the configuration files.
    /// Defaults to "./data" if not specified.
    #[schema(example = "./data")]
    pub data_directory: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[schema(example = json!({
    "vendor": "TestVendor",
    "model": "TestModel_4",
    "serial": "CP_4_RESTOnly",
    "protocol": "ocpp1.6",
    "evses": [
        { "evse_index": 1, "connector_count": 2 },
        { "evse_index": 2, "connector_count": 2 }
    ],
    "configuration": [
        {
            "name": "OCPPCommCtrlr",
            "variables": [
                {
                    "name": "MeterValueSampleInterval",
                    "value": "10",
                    "required": true
                }
            ]
        }
    ]
}))]
pub struct CreateCPFromRESTRequest {
    /// Optional: Username for OCPP authentication (only required for security profile 6)
    #[serde(default)]
    #[schema(example = "CP_4_User")]
    pub username: Option<String>,

    /// Optional: Password for OCPP authentication (only required for security profile 6)
    #[serde(default)]
    #[schema(example = "CP_4_Pass")]
    pub password: Option<String>,

    /// **Required**: Vendor name of the charge point
    #[schema(example = "TestVendor")]
    pub vendor: String,

    /// **Required**: Model name of the charge point
    #[schema(example = "TestModel_4")]
    pub model: String,

    /// **Required**: Unique serial number for the charge point
    #[schema(example = "CP_4_RESTOnly")]
    pub serial: String,

    /// **Required**: OCPP protocol version ("ocpp1.6" or "ocpp2.0.1")
    #[schema(example = "ocpp1.6")]
    pub protocol: String,

    /// **Required**: EVSE configuration. Each item defines one EVSE and how many connectors it has.
    #[schema(min_items = 1, example = json!([
        { "evse_index": 1, "connector_count": 2 },
        { "evse_index": 2, "connector_count": 2 }
    ]))]
    pub evses: Vec<CreateCPRESTEVSERequest>,

    /// Optional device model configuration variables for the charge point.
    #[serde(default)]
    pub configuration: Option<Vec<CreateCPRESTConfigurationComponent>>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[schema(example = json!({
    "evse_index": 1,
    "connector_count": 2
}))]
pub struct CreateCPRESTEVSERequest {
    /// EVSE index number. Must be unique within this charge point.
    #[schema(example = 1)]
    pub evse_index: u32,

    /// Number of connectors for this EVSE.
    #[schema(example = 2)]
    pub connector_count: u32,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[schema(example = json!({
    "name": "OCPPCommCtrlr",
    "variables": [
        {
            "name": "MeterValueSampleInterval",
            "value": "10",
            "required": true
        }
    ]
}))]
pub struct CreateCPRESTConfigurationComponent {
    /// Optional component instance name.
    #[serde(default)]
    #[schema(example = "OCPPCommCtrlr")]
    pub instance: Option<String>,

    /// Name of the device model component.
    #[schema(example = "OCPPCommCtrlr")]
    pub name: String,

    /// Optional EVSE index this component belongs to.
    #[serde(default)]
    #[schema(example = 1)]
    pub evse: Option<u32>,

    /// Optional connector index this component belongs to.
    #[serde(default)]
    #[schema(example = 1)]
    pub connector: Option<u32>,

    /// Configuration variables for this component.
    pub variables: Vec<CreateCPRESTConfigurationVariable>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[schema(example = json!({
    "name": "MeterValueSampleInterval",
    "value": "10",
    "required": true
}))]
pub struct CreateCPRESTConfigurationVariable {
    /// Variable name.
    #[schema(example = "MeterValueSampleInterval")]
    pub name: String,

    /// Value for the variable.
    #[schema(example = "10")]
    pub value: String,

    /// Whether this variable is required.
    #[serde(default)]
    #[schema(example = true)]
    pub required: Option<bool>,

    /// Optional OCPP 1.6 key to use for this variable.
    #[serde(default)]
    #[schema(example = "OCPPCommCtrlrMeterValueSampleInterval")]
    pub ocpp16_key: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[schema(example = json!({
    "components": [
        {
            "name": "AlignedDataCtrlr",
            "variables": [
                {
                    "name": "Enabled",
                    "value": true
                }
            ]
        }
    ]
}))]
pub struct UpdateCPConfigurationRequest {
    /// Configuration components to update
    pub components: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(example = json!({
    "cp_id": 0,
    "username": "CP_1_User",
    "password": "CP_1_Pass",
    "vendor": "TestVendor",
    "model": "TestModel_1",
    "serial": "CP_1_Full",
    "protocol": "ocpp1.6",
    "booted": false,
    "events_count": 5,
    "total_energy_delivered_kwh": 0.0
}))]
pub struct CPDataResponse {
    /// Unique charge point identifier
    pub cp_id: usize,

    /// OCPP authentication username
    pub username: Option<String>,

    /// OCPP authentication password
    pub password: Option<String>,

    /// Charge point vendor name
    pub vendor: String,

    /// Charge point model name
    pub model: String,

    /// Unique serial number
    pub serial: String,

    /// OCPP protocol version
    pub protocol: Option<String>,

    /// Whether the charge point has completed boot sequence
    pub booted: bool,

    /// Number of scheduled events
    pub events_count: usize,

    /// Total energy delivered in kWh
    pub total_energy_delivered_kwh: f32,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CPConfigurationResponse {
    /// Configuration components
    pub components: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[schema(example = json!({
    "delay": 0,
    "event_type": "authorize",
    "id_tag": "TAG12345"
}))]
pub struct AddEventRequest {
    /// Delay in seconds from start of event processing to trigger the event (0 = trigger immediately)
    #[schema(example = 0)]
    pub delay: u32,

    /// Type of event to trigger. Determines which additional fields are required.
    /// - "authorize": requires `id_tag`
    /// - "plug": requires `evse_index`, `connector_id`, `ev_power`, `ev_capacity`, `ev_soc`, `ev_final_soc`
    /// - "unplug": requires `evse_index`, `connector_id`
    /// - "faulted"/"fault_cleared": optional `connector_ids` (empty = all connectors)
    /// - "local_stop", "start_transaction", "stop_transaction", "communication_stop", "communication_start": no additional fields
    #[schema(example = "authorize")]
    pub event_type: String,

    /// RFID tag ID (required for "authorize" events)
    #[serde(default)]
    #[schema(example = "TAG12345")]
    pub id_tag: Option<String>,

    /// EVSE index (required for "plug" and "unplug" events)
    #[serde(default)]
    #[schema(example = 1)]
    pub evse_index: Option<u32>,

    /// Connector ID (required for "plug" and "unplug" events)
    #[serde(default)]
    #[schema(example = 1)]
    pub connector_id: Option<u32>,

    /// EV charging power in kW (required for "plug" events)
    #[serde(default)]
    #[schema(example = 7.4)]
    pub ev_power: Option<f32>,

    /// EV battery capacity in kWh (required for "plug" events)
    #[serde(default)]
    #[schema(example = 60.0)]
    pub ev_capacity: Option<f32>,

    /// EV current state of charge (0-100%) (required for "plug" events)
    #[serde(default)]
    #[schema(example = 20.0)]
    pub ev_soc: Option<f32>,

    /// EV target state of charge (0-100%) (required for "plug" events)
    #[serde(default)]
    #[schema(example = 80.0)]
    pub ev_final_soc: Option<f32>,

    /// Connector IDs to affect (for "faulted"/"fault_cleared" events, empty = all connectors)
    #[serde(default)]
    #[schema(example = json!([1, 2]))]
    pub connector_ids: Option<Vec<u32>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[schema(example = json!([
    {
        "delay": 0,
        "event_type": "authorize",
        "id_tag": "TAG12345"
    },
    {
        "delay": 2,
        "event_type": "plug",
        "evse_index": 1,
        "connector_id": 1,
        "ev_power": 7.4,
        "ev_capacity": 60.0,
        "ev_soc": 20.0,
        "ev_final_soc": 80.0
    },
    {
        "delay": 600,
        "event_type": "unplug",
        "evse_index": 1,
        "connector_id": 1
    },
    {
        "delay": 300,
        "event_type": "faulted",
        "connector_ids": [1]
    },
    {
        "delay": 0,
        "event_type": "communication_stop"
    }
]))]
pub struct AddMultipleEventsRequest {
    /// List of events to add in sequence. Each event follows the same schema as individual event requests.
    pub events: Vec<AddEventRequest>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AddEventsToMultipleCPsRequest {
    /// Map of CP IDs to their event lists. Each CP will have all events added to it.
    /// Example: {"1": [{...event1...}, {...event2...}], "2": [{...event3...}]}
    #[serde(flatten)]
    pub events_by_cp: HashMap<usize, Vec<AddEventRequest>>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// RFID authorization event
    Authorize,
    /// Local stop charging event
    LocalStop,
    /// Plug in EV event
    Plug,
    /// Unplug EV event
    Unplug,
    /// Remote start charging event
    RemoteStart,
    /// Remote stop charging event
    RemoteStop,
    /// Stop OCPP communication
    CommunicationStop,
    /// Start OCPP communication
    CommunicationStart,
    /// Fault event on connectors
    Faulted,
    /// Clear fault on connectors
    FaultCleared,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AuthorizeEventData {
    /// RFID tag ID for authorization
    #[schema(example = "TAG12345")]
    pub id_tag: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PlugEventData {
    /// EVSE index (typically 1)
    #[schema(example = 1)]
    pub evse_index: u32,

    /// Connector ID on the EVSE
    #[schema(example = 1)]
    pub connector_id: u32,

    /// EV charging power in kW
    #[schema(example = 7.4)]
    pub ev_power: f32,

    /// EV battery capacity in kWh
    #[schema(example = 60.0)]
    pub ev_capacity: f32,

    /// EV current state of charge (0-100%)
    #[schema(example = 20.0)]
    pub ev_soc: f32,

    /// EV target state of charge (0-100%)
    #[schema(example = 80.0)]
    pub ev_final_soc: f32,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UnplugEventData {
    /// EVSE index
    #[schema(example = 1)]
    pub evse_index: u32,

    /// Connector ID on the EVSE
    #[schema(example = 1)]
    pub connector_id: u32,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct FaultEventData {
    /// Connector IDs to fault (empty array = all connectors)
    #[schema(example = json!([1, 2]))]
    pub connector_ids: Vec<u32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CreateCPResponse {
    pub cp_id: usize,
    pub message: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CreateMultipleCPsResponse {
    /// List of created CP IDs
    pub cp_ids: Vec<usize>,
    pub message: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub details: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateCPRequest {
    pub name: String,
    #[serde(default)]
    pub use_original: Option<bool>,
    pub data_directory: String,
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
    pub message_tracker: Arc<MessageTracker>,
    pub next_id: AtomicUsize,
}

impl CPStore {
    pub fn new() -> Self {
        Self {
            cps: tokio::sync::RwLock::new(HashMap::new()),
            cp_instances: tokio::sync::RwLock::new(HashMap::new()),
            cp_tasks: tokio::sync::RwLock::new(HashMap::new()),
            event_channels: tokio::sync::RwLock::new(HashMap::new()),
            message_tracker: Arc::new(MessageTracker::new()),
            next_id: AtomicUsize::new(0),
        }
    }

    pub async fn create_cp(&self, request: CreateCPRequest) -> usize {
        let cp_id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let cp_file_name = request.name.clone();
        let use_original = request.use_original.unwrap_or(false);

        let (cp_config, variables) = CP::prepare_cp_variables_files(&request.data_directory, &cp_file_name, use_original)
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
            message_tracker: Some(self.message_tracker.clone()),
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

    pub async fn create_cp_from_rest(&self, request: CreateCPFromRESTRequest) -> usize {
        let cp_id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let mut evses = HashMap::new();
        for evse_request in request.evses.iter() {
            let connector_ids: Vec<u32> = (1..=evse_request.connector_count).collect();
            evses.insert(
                evse_request.evse_index,
                crate::cp_data::EVSE {
                    is_ac: true,
                    connector_ids,
                    plugged_in: None,
                    started: false,
                    ev: None,
                    charging_task: None,
                    meter_energy: 0.0,
                    status: "Available".to_string(),
                    faulted_connectors: vec![],
                    current_tariff: None,
                    idle_tariff: None,
                    running_cost: 0.0,
                    last_cost_update: None,
                    triggers: None,
                    next_period: None,
                    pricing_state: None,
                },
            );
        }

        let variables = if let Some(configuration_components) = request.configuration.clone() {
            let mut components: Vec<crate::cp_data::Components> = configuration_components
                .into_iter()
                .map(|component| {
                    let variables = component
                        .variables
                        .into_iter()
                        .map(|variable| {
                            let ocpp16_key = variable.ocpp16_key.or_else(|| {
                                Some(format!(
                                    "{}{}",
                                    component.name,
                                    variable.name
                                ))
                            });

                            let value = variable.value.clone();

                            crate::cp_data::Variable {
                                instance: None,
                                name: variable.name,
                                ocpp16_key,
                                value: Some(value.clone()),
                                required: variable.required.unwrap_or(false),
                                attributes: crate::cp_data::Attributes {
                                    mutability: "Writable".to_string(),
                                    type_string: Some("String".to_string()),
                                },
                                characteristics: crate::cp_data::Characteristics {
                                    data_type: "String".to_string(),
                                    unit: None,
                                    values_list: None,
                                    max_limit: None,
                                    min_limit: None,
                                },
                                used: None,
                                default_value: Some(crate::cp_data::ValueType::String(value)),
                            }
                        })
                        .collect();

                    crate::cp_data::Components {
                        instance: component.instance,
                        name: component.name,
                        evse: component.evse,
                        connector: component.connector,
                        variables,
                    }
                })
                .collect();

            for component in components.iter_mut() {
                let mut component_string = component.name.clone();
                if let Some(evse) = component.evse {
                    component_string.push_str(&evse.to_string());
                }
                if let Some(instance) = &component.instance {
                    component_string.push_str(instance);
                }
                for variable in component.variables.iter_mut() {
                    if variable.ocpp16_key.is_none() {
                        variable.ocpp16_key = Some(component_string.clone() + variable.name.as_str());
                    }
                }
            }

            crate::cp_data::GetVariableData { components }
        } else {
            crate::cp_data::GetVariableData {
                components: vec![
                    crate::cp_data::Components {
                        instance: None,
                        name: "AlignedDataCtrlr".to_string(),
                        evse: None,
                        connector: None,
                        variables: vec![],
                    },
                    crate::cp_data::Components {
                        instance: None,
                        name: "OCPPCommCtrlr".to_string(),
                        evse: None,
                        connector: None,
                        variables: vec![],
                    },
                ],
            }
        };

        let cp_data = CPData {
            username: request.username.clone(),
            password: request.password.clone(),
            supported_protocol_versions: Some("ocpp1.6, ocpp2.0.1".to_string()),
            selected_protocol: Some(request.protocol.clone()),
            serial: request.serial.clone(),
            model: request.model.clone(),
            vendor: request.vendor.clone(),
            booted: false,
            evses,
            events: vec![],
            authorization: None,
            variables,
            base_url: "ws://127.0.0.1:8180/steve/websocket/CentralSystemService/".to_string(),
            variables_file_name: String::new(), // No file associated
            use_original: false,
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
            message_tracker: Some(self.message_tracker.clone()),
        };

        let task = tokio::spawn(async move {
            CP_ID
                .scope(cp_id, async {
                    info!(
                        "Starting CP {} with serial {} now (REST-created)",
                        cp_id,
                        cp.data.lock().await.serial
                    );
                    cp.run().await
                })
                .await
        });

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
        connector_ids: Option<Vec<u32>>,
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
                "faulted" => {
                    let ids = connector_ids.unwrap_or_default();
                    EventTypes::Faulted(ids)
                }
                "fault_cleared" => {
                    let ids = connector_ids.unwrap_or_default();
                    EventTypes::FaultCleared(ids)
                }
                "remote_start" => EventTypes::RemoteStart(ChargeSessionReference {
                    evse_index: evse_index.unwrap_or(1),
                    connector_id: connector_id.unwrap_or(1),
                }),
                "remote_stop" => EventTypes::RemoteStop(ChargeSessionReference {
                    evse_index: evse_index.unwrap_or(1),
                    connector_id: connector_id.unwrap_or(1),
                }),
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

            Ok(())
        } else {
            Err(format!("CP with ID {} not found", cp_id))
        }
    }
}

#[utoipa::path(
    post,
    path = "/cp/from-file",
    tag = "Charge Points",
    operation_id = "create_cp_from_file",
    request_body = CreateCPFromFileRequest,
    responses(
        (status = 201, description = "Charge point created successfully", body = CreateCPResponse),
        (status = 500, description = "Internal server error - check file paths and formats", body = ErrorResponse)
    )
)]
async fn create_cp_from_file_handler(
    State(store): State<Arc<CPStore>>,
    Json(payload): Json<CreateCPFromFileRequest>,
) -> impl IntoResponse {
    let cp_request = CreateCPRequest {
        name: payload.name,
        use_original: payload.use_original,
        data_directory: payload.data_directory,
    };
    let cp_id = store.create_cp(cp_request).await;
    (
        StatusCode::CREATED,
        Json(CreateCPResponse {
            cp_id,
            message: format!("CP {} created successfully from file", cp_id),
        }),
    )
}

#[utoipa::path(
    post,
    path = "/cp/from-rest",
    tag = "Charge Points",
    operation_id = "create_cp_from_rest",
    request_body = CreateCPFromRESTRequest,
    responses(
        (status = 201, description = "Charge point created successfully", body = CreateCPResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn create_cp_from_rest_handler(
    State(store): State<Arc<CPStore>>,
    Json(payload): Json<CreateCPFromRESTRequest>,
) -> impl IntoResponse {
    let cp_id = store.create_cp_from_rest(payload).await;
    (
        StatusCode::CREATED,
        Json(CreateCPResponse {
            cp_id,
            message: format!("CP {} created successfully from REST parameters", cp_id),
        }),
    )
}

#[utoipa::path(
    post,
    path = "/cp/from-rest/bulk",
    tag = "Charge Points",
    operation_id = "create_multiple_cps",
    request_body = Vec::<CreateCPFromRESTRequest>,
    responses(
        (status = 201, description = "Charge points created successfully", body = CreateMultipleCPsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn create_multiple_cps_handler(
    State(store): State<Arc<CPStore>>,
    Json(payloads): Json<Vec<CreateCPFromRESTRequest>>,
) -> impl IntoResponse {
    let mut cp_ids = Vec::new();
    for payload in payloads {
        let cp_id = store.create_cp_from_rest(payload).await;
        cp_ids.push(cp_id);
    }
    (
        StatusCode::CREATED,
        Json(CreateMultipleCPsResponse {
            cp_ids: cp_ids.clone(),
            message: format!("Created {} charge points successfully", cp_ids.len()),
        }),
    )
}

#[utoipa::path(
    get,
    path = "/cp",
    tag = "Charge Points",
    operation_id = "list_charge_points",
    responses(
        (status = 200, description = "List of charge points", body = [CPDataResponse], example = json!([
            {
                "cp_id": 0,
                "username": "CP_1_User",
                "password": "CP_1_Pass",
                "vendor": "TestVendor",
                "model": "TestModel_1",
                "serial": "CP_1_Full",
                "protocol": "ocpp1.6",
                "booted": false,
                "events_count": 5,
                "total_energy_delivered_kwh": 0.0
            }
        ]))
    )
)]
async fn list_cps_handler(State(store): State<Arc<CPStore>>) -> impl IntoResponse {
    let cps = store.list_cps().await;
    let cp_list: Vec<CPDataResponse> = cps
        .into_iter()
        .enumerate()
        .map(|(index, cp)| CPDataResponse {
            cp_id: index,
            username: cp.username,
            password: cp.password,
            vendor: cp.vendor,
            model: cp.model,
            serial: cp.serial,
            protocol: cp.selected_protocol,
            booted: cp.booted,
            events_count: cp.events.len(),
            total_energy_delivered_kwh: 0.0, // TODO: Calculate from actual data
        })
        .collect();
    Json(cp_list).into_response()
}

#[utoipa::path(
    get,
    path = "/cp/{cp_id}",
    tag = "Charge Points",
    operation_id = "get_charge_point",
    params(
        ("cp_id" = usize, Path, description = "Charge point ID")
    ),
    responses(
        (status = 200, description = "Charge point details", body = CPDataResponse),
        (status = 404, description = "Charge point not found", body = ErrorResponse)
    )
)]
async fn get_cp_data_handler(
    State(store): State<Arc<CPStore>>,
    Path(cp_id): Path<usize>,
) -> impl IntoResponse {
    match store.get_cp(cp_id).await {
        Some(cp) => Json(CPDataResponse {
            cp_id,
            username: cp.username,
            password: cp.password,
            vendor: cp.vendor,
            model: cp.model,
            serial: cp.serial,
            protocol: cp.selected_protocol,
            booted: cp.booted,
            events_count: cp.events.len(),
            total_energy_delivered_kwh: 0.0, // TODO: Calculate from actual data
        })
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Charge point not found".to_string(),
                details: Some(format!("No charge point with ID {}", cp_id)),
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/cp/{cp_id}/configuration",
    tag = "Charge Points",
    operation_id = "get_charge_point_configuration",
    params(
        ("cp_id" = usize, Path, description = "Charge point ID")
    ),
    responses(
        (status = 200, description = "Configuration data", body = CPConfigurationResponse),
        (status = 404, description = "Charge point not found", body = ErrorResponse)
    )
)]
async fn get_cp_configuration_handler(
    State(store): State<Arc<CPStore>>,
    Path(cp_id): Path<usize>,
) -> impl IntoResponse {
    match store.get_cp(cp_id).await {
        Some(cp) => Json(CPConfigurationResponse {
            components: vec![], // TODO: Return actual configuration data
        })
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Charge point not found".to_string(),
                details: Some(format!("No charge point with ID {}", cp_id)),
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(
    put,
    path = "/cp/{cp_id}/configuration",
    tag = "Charge Points",
    operation_id = "update_charge_point_configuration",
    params(
        ("cp_id" = usize, Path, description = "Charge point ID")
    ),
    request_body = UpdateCPConfigurationRequest,
    responses(
        (status = 200, description = "Configuration updated successfully"),
        (status = 404, description = "Charge point not found", body = ErrorResponse)
    )
)]
async fn update_cp_configuration_handler(
    State(store): State<Arc<CPStore>>,
    Path(cp_id): Path<usize>,
    Json(_payload): Json<UpdateCPConfigurationRequest>,
) -> impl IntoResponse {
    match store.get_cp(cp_id).await {
        Some(_cp) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "message": "Configuration updated successfully",
                "cp_id": cp_id
            })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Charge point not found".to_string(),
                details: Some(format!("No charge point with ID {}", cp_id)),
            }),
        )
            .into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/cp/{cp_id}/events",
    tag = "Events",
    params(
        ("cp_id" = usize, Path, description = "Charge Point ID")
    ),
    request_body = AddEventRequest,
    responses(
        (status = 201, description = "Event added successfully", body = serde_json::Value),
        (status = 400, description = "Bad request", body = ErrorResponse)
    )
)]
async fn add_event_handler(
    State(store): State<Arc<CPStore>>,
    Path(cp_id): Path<usize>,
    Json(payload): Json<AddEventRequest>,
) -> impl IntoResponse {
    match store
        .add_event(
            cp_id,
            payload.delay,
            &payload.event_type,
            payload.id_tag,
            payload.evse_index,
            payload.connector_id,
            None, // power_vs_soc - calculated from ev_power
            payload.ev_soc,
            payload.ev_final_soc,
            payload.ev_capacity,
            payload.connector_ids,
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

#[utoipa::path(
    post,
    path = "/cp/{cp_id}/events/bulk",
    tag = "Events",
    params(
        ("cp_id" = usize, Path, description = "Charge Point ID")
    ),
    request_body = AddMultipleEventsRequest,
    responses(
        (status = 201, description = "Events added successfully", body = serde_json::Value),
        (status = 400, description = "Bad request", body = ErrorResponse)
    )
)]
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
                event.delay,
                &event.event_type,
                event.id_tag,
                event.evse_index,
                event.connector_id,
                None, // power_vs_soc
                event.ev_soc,
                event.ev_final_soc,
                event.ev_capacity,
                event.connector_ids,
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

#[utoipa::path(
    post,
    path = "/cp/events",
    tag = "Events",
    operation_id = "add_events_to_multiple_cps",
    request_body = AddEventsToMultipleCPsRequest,
    responses(
        (status = 201, description = "Events added to charge points successfully", body = serde_json::Value),
        (status = 400, description = "Bad request", body = ErrorResponse)
    )
)]
async fn add_events_to_multiple_cps_handler(
    State(store): State<Arc<CPStore>>,
    Json(payload): Json<AddEventsToMultipleCPsRequest>,
) -> impl IntoResponse {
    let mut results = serde_json::json!({});
    let mut total_added = 0;
    let mut total_errors = 0;

    for (cp_id, events) in payload.events_by_cp {
        let mut cp_errors = Vec::new();
        let mut cp_count = 0;

        for event in events {
            match store
                .add_event(
                    cp_id,
                    event.delay,
                    &event.event_type,
                    event.id_tag,
                    event.evse_index,
                    event.connector_id,
                    None, // power_vs_soc
                    event.ev_soc,
                    event.ev_final_soc,
                    event.ev_capacity,
                    event.connector_ids,
                )
                .await
            {
                Ok(_) => {
                    cp_count += 1;
                    total_added += 1;
                }
                Err(e) => {
                    cp_errors.push(e);
                    total_errors += 1;
                }
            }
        }

        results[cp_id.to_string()] = serde_json::json!({
            "events_added": cp_count,
            "errors": cp_errors
        });
    }

    if total_errors == 0 {
        (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "message": format!("Added {} events to multiple charge points", total_added),
                "results": results,
                "total_added": total_added
            })),
        )
            .into_response()
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Failed to add some events".to_string(),
                details: Some(format!("Added {} events with {} errors", total_added, total_errors)),
            }),
        )
            .into_response()
    }
}

#[utoipa::path(
    get,
    path = "/summary",
    tag = "Statistics",
    responses(
        (status = 200, description = "Message statistics summary", body = TestSummary)
    )
)]
async fn get_summary_handler(State(store): State<Arc<CPStore>>) -> impl IntoResponse {
    let summary = store.message_tracker.get_summary().await;
    Json(summary).into_response()
}

pub async fn start_rest_server() -> tokio::task::JoinHandle<Result<(), std::io::Error>> {
    tokio::spawn(async {
        let store = Arc::new(CPStore::new());
        let app = Router::new()
            .route("/cp/from-file", post(create_cp_from_file_handler))
            .route("/cp/from-rest", post(create_cp_from_rest_handler))
            .route("/cp/from-rest/bulk", post(create_multiple_cps_handler))
            .route("/cp", get(list_cps_handler))
            .route("/cp/:cp_id", get(get_cp_data_handler))
            .route("/cp/:cp_id/configuration", get(get_cp_configuration_handler).put(update_cp_configuration_handler))
            .route("/cp/:cp_id/events", post(add_event_handler))
            .route("/cp/:cp_id/events/bulk", post(add_multiple_events_handler))
            .route("/cp/events", post(add_events_to_multiple_cps_handler))
            .route("/summary", get(get_summary_handler))
            .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
            .with_state(store);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
            .await
            .expect("Failed to bind REST API to port 3000");

        println!("REST API Server running on http://127.0.0.1:3000");
        println!("Swagger UI available at http://127.0.0.1:3000/swagger-ui");
        println!("Try: curl -X POST http://localhost:3000/cp/from-file -H 'Content-Type: application/json' -d '{{\"name\":\"example_1\",\"data_directory\":\"./data\"}}'\n");

        axum::serve(listener, app).await
    })
}
