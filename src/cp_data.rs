use serde;
use std::collections::HashMap;
use std::fmt;

#[derive(Clone, Debug)]
pub struct RFID {
    pub id_tag: String,
}

#[derive(Clone, Debug)]
pub struct PlugAndCharge {}

#[derive(Clone, Debug)]
pub enum AuthorizationType {
    RFID(RFID),
    PlugAndCharge(PlugAndCharge),
    Remote(RFID),
}

#[derive(Clone, Debug)]

pub struct EV {
    pub power_vs_soc: Vec<(f32, f32)>, //kW vs SOC
    pub soc: f32,
    pub final_soc: f32,
    pub capacity: f32, //kWh
    pub power: f32,    //kW

                       //todo thermal stuff
                       //todo efficiency
}

#[derive(Clone, Debug)]
pub enum EventTypes {
    Authorize(AuthorizationType),
    LocalStop,
    Plug(ChargeSessionReference, EV),
    Unplug(ChargeSessionReference),
    RemoteStart(ChargeSessionReference),
    RemoteStop(ChargeSessionReference),
    CommunicationStop,
    CommunicationStart,
}
#[derive(Clone)]

pub struct ScheduledEvents {
    pub duration: u32,

    pub event: EventTypes,
}

#[derive(Clone)]

pub struct ChargeSession {
    pub ev: EV,
    pub authorization: Option<AuthorizationType>,
    pub started: bool,
    pub plugged_in: bool,
}

#[derive(Clone, Debug)]
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

#[derive(Clone)]

pub struct EVSE {
    pub is_ac: bool,
    pub connector_ids: Vec<u32>,
    pub plugged_in: Option<u32>,
    pub started: bool,
    pub ev: Option<EV>,
    pub charging_task: Option<tokio::task::AbortHandle>,
    pub meter_energy: f32, // Energy in Wh
    pub status: String, // OCPP status: Available, Preparing, Charging, etc.
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]

pub struct GetVariableData {
    pub components: Vec<Components>,
}
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]

pub struct Components {
    pub instance: Option<String>,
    pub name: String,
    pub evse: Option<u32>,
    pub connector: Option<u32>,
    pub variables: Vec<Variable>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]

pub enum ValueType {
    String(String),
    Integer(i32),
    Boolean(bool),
    // Add more types as needed
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

#[derive(Clone, Debug, serde::Deserialize)]
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
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]

pub struct Attributes {
    pub mutability: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    type_string: Option<String>,
}
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
pub struct Characteristics {
    data_type: String,
    unit: Option<String>,
    values_list: Option<String>,
    max_limit: Option<u32>,
    min_limit: Option<u32>,
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
}
