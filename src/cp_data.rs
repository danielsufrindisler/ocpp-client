use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::time::Instant;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChargingPrice {
    #[serde(rename = "kWhPrice")]
    pub kwh_price: Option<f64>,
    #[serde(rename = "hourPrice")]
    pub hour_price: Option<f64>,
    #[serde(rename = "flatFee")]
    pub flat_fee: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdlePrice {
    #[serde(rename = "graceMinutes")]
    pub grace_minutes: Option<i32>,
    #[serde(rename = "hourPrice")]
    pub hour_price: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NextPeriod {
    #[serde(rename = "atTime")]
    pub at_time: String, // dateTime
    #[serde(rename = "chargingPrice")]
    pub charging_price: ChargingPrice,
    #[serde(rename = "idlePrice")]
    pub idle_price: Option<IdlePrice>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Triggers {
    #[serde(rename = "atTime")]
    pub at_time: Option<String>,
    #[serde(rename = "atEnergykWh")]
    pub at_energy_kwh: Option<f64>,
    #[serde(rename = "atPowerkW")]
    pub at_power_kw: Option<f64>,
    #[serde(rename = "atCPStatus")]
    pub at_cp_status: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DefaultPrice {
    #[serde(rename = "priceText")]
    pub price_text: String,
    #[serde(rename = "priceTextOffline")]
    pub price_text_offline: Option<String>,
    #[serde(rename = "chargingPrice")]
    pub charging_price: Option<ChargingPrice>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetUserPriceData {
    #[serde(rename = "idToken")]
    pub id_token: String,
    #[serde(rename = "priceText")]
    pub price_text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunningCostData {
    #[serde(rename = "transactionId")]
    pub transaction_id: i32,
    pub timestamp: String,
    #[serde(rename = "meterValue")]
    pub meter_value: i32,
    pub cost: f64,
    pub state: String, // "Charging" or "Idle"
    #[serde(rename = "chargingPrice")]
    pub charging_price: Option<ChargingPrice>,
    #[serde(rename = "idlePrice")]
    pub idle_price: Option<IdlePrice>,
    #[serde(rename = "nextPeriod")]
    pub next_period: Option<NextPeriod>,
    #[serde(rename = "triggerMeterValue")]
    pub trigger_meter_value: Option<Triggers>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FinalCostData {
    #[serde(rename = "transactionId")]
    pub transaction_id: i32,
    pub cost: f64,
    #[serde(rename = "priceText")]
    pub price_text: String,
    #[serde(rename = "qrCodeText")]
    pub qr_code_text: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectorUnpluggedData {
    #[serde(rename = "transactionId")]
    pub transaction_id: i32,
    pub timestamp: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct RFID {
    pub id_tag: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct PlugAndCharge {}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum AuthorizationType {
    RFID(RFID),
    PlugAndCharge(PlugAndCharge),
    Remote(RFID),
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]

pub struct EV {
    pub power_vs_soc: Vec<(f32, f32)>, //kW vs SOC
    pub soc: f32,
    pub final_soc: f32,
    pub capacity: f32, //kWh
    pub power: f32,    //kW

                       //todo thermal stuff
                       //todo efficiency
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum EventTypes {
    Authorize(AuthorizationType),
    LocalStop,
    Plug(ChargeSessionReference, EV),
    Unplug(ChargeSessionReference),
    RemoteStart(ChargeSessionReference),
    RemoteStop(ChargeSessionReference),
    CommunicationStop,
    CommunicationStart,
    Faulted(Vec<u32>),  // Empty vec means all connectors, otherwise specific connector IDs
    FaultCleared(Vec<u32>), // Empty vec means all connectors, otherwise specific connector IDs
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]

pub struct ScheduledEvents {
    pub duration: u32,

    pub event: EventTypes,
}

/// Event with absolute scheduled time (for internal use in event loop)
#[derive(Clone, Debug)]
pub struct ScheduledEventWithAbsoluteTime {
    pub scheduled_time: Instant,
    pub event: EventTypes,
    pub event_index: usize,
}

/// Implement ordering for BinaryHeap (reverse order = min-heap behavior)
impl PartialEq for ScheduledEventWithAbsoluteTime {
    fn eq(&self, other: &Self) -> bool {
        self.scheduled_time == other.scheduled_time
    }
}

impl Eq for ScheduledEventWithAbsoluteTime {}

impl PartialOrd for ScheduledEventWithAbsoluteTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledEventWithAbsoluteTime {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap (earlier times have higher priority)
        other.scheduled_time.cmp(&self.scheduled_time)
    }
}

/// Message sent on event channel to inject new events dynamically
#[derive(Clone, Debug)]
pub enum EventInjectionMessage {
    /// Insert a new event to be processed at scheduled_time
    InsertEvent {
        event: EventTypes,
        duration_secs: u32,
    },
    /// Stop the event loop gracefully
    Stop,
}

#[derive(Clone)]

pub struct ChargeSession {
    pub ev: EV,
    pub authorization: Option<AuthorizationType>,
    pub started: bool,
    pub plugged_in: bool,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ChargeSessionReference {
    pub evse_index: u32,
    pub connector_id: u32,
}

#[derive(Clone)]
pub enum MessageReference {
    ChargeSession(ChargeSessionReference),
    EventIndex(usize),
    NoReference,
}

pub struct EVSE {
    pub is_ac: bool,
    pub connector_ids: Vec<u32>,
    pub plugged_in: Option<u32>,
    pub started: bool,
    pub ev: Option<EV>,
    pub charging_task: Option<tokio::task::JoinHandle<()>>,
    pub meter_energy: f32, // Energy in Wh
    pub status: String,    // OCPP status: Available, Preparing, Charging, etc.
    pub faulted_connectors: Vec<u32>, // Connectors in Faulted state
    // Pricing fields
    pub current_tariff: Option<ChargingPrice>,
    pub idle_tariff: Option<IdlePrice>,
    pub running_cost: f64,
    pub last_cost_update: Option<String>, // timestamp
    pub triggers: Option<Triggers>,
    pub next_period: Option<NextPeriod>,
    pub pricing_state: Option<String>, // "Charging" or "Idle"
}

impl Clone for EVSE {
    fn clone(&self) -> Self {
        EVSE {
            is_ac: self.is_ac,
            connector_ids: self.connector_ids.clone(),
            plugged_in: self.plugged_in,
            started: self.started,
            ev: self.ev.clone(),
            charging_task: None, // Don't clone the JoinHandle
            meter_energy: self.meter_energy,
            status: self.status.clone(),
            faulted_connectors: self.faulted_connectors.clone(),
            current_tariff: self.current_tariff.clone(),
            idle_tariff: self.idle_tariff.clone(),
            running_cost: self.running_cost,
            last_cost_update: self.last_cost_update.clone(),
            triggers: self.triggers.clone(),
            next_period: self.next_period.clone(),
            pricing_state: self.pricing_state.clone(),
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]

pub struct GetVariableData {
    pub components: Vec<Components>,
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]

pub struct Components {
    pub instance: Option<String>,
    pub name: String,
    pub evse: Option<u32>,
    pub connector: Option<u32>,
    pub variables: Vec<Variable>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]

pub enum ValueType {
    String(String),
    Integer(i32),
    Boolean(bool),
    // Add more types as needed
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]

pub struct Variable {
    pub instance: Option<String>,
    pub name: String,
    pub ocpp16_key: Option<String>,
    pub value: Option<String>,
    pub required: bool,
    pub attributes: Attributes,
    pub characteristics: Characteristics,
    pub used: Option<bool>,
    pub default_value: Option<ValueType>,
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]

pub struct Attributes {
    pub mutability: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub type_string: Option<String>,
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
pub struct Characteristics {
    pub data_type: String,
    pub unit: Option<String>,
    pub values_list: Option<String>,
    pub max_limit: Option<u32>,
    pub min_limit: Option<u32>,
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ValueType::String(s) => write!(f, "{}", s),
            ValueType::Integer(i) => write!(f, "{}", i),
            ValueType::Boolean(b) => write!(f, "{}", b),
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct CPFileInfo {
    pub username: Option<String>,
    pub password: Option<String>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub protocol: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct CPConfigFile {
    #[serde(default)]
    pub cp: Option<CPFileInfo>,
    #[serde(default)]
    pub events: Vec<ScheduledEvents>,
    #[serde(default)]
    pub components: Vec<Components>,
}

#[derive(Clone)]
pub struct CPData {
    pub username: Option<String>,
    pub password: Option<String>,
    pub supported_protocol_versions: Option<String>,
    pub selected_protocol: Option<String>, // Selected protocol before connection: "ocpp1.6" or "ocpp2.0.1"
    pub serial: String,
    pub model: String,
    pub vendor: String,
    pub booted: bool,
    pub evses: HashMap<u32, EVSE>,
    pub events: Vec<ScheduledEvents>,
    pub authorization: Option<AuthorizationType>, // a pending authorization that is not tied to a specific connector
    pub variables: GetVariableData,
    pub base_url: String,
    pub variables_file_name: String, // file name without extension, e.g. "0"
    pub use_original: bool,
}
