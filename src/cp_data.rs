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

#[derive(Clone)]

pub struct EV {
    pub power_vs_soc: Vec<(f32, f32)>,
    //todo thermal stuff
    //todo efficiency
}

#[derive(Clone, Debug)]
pub enum EventTypes {
    Authorize(AuthorizationType),
    LocalStop,
    Plug(ChargeSessionReference),
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
    pub evse_index: usize,
    pub connector_id: u32,
    pub charge_session_index: usize,
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
    pub charge_sessions: Vec<ChargeSession>,
}

pub struct CPData {
    pub username: Option<String>,
    pub password: Option<String>,
    pub supported_protocol_versions: Option<String>,
    pub selected_protocol: Option<String>, // Selected protocol before connection: "ocpp1.6" or "ocpp2.0.1"
    pub serial: String,
    pub model: String,
    pub vendor: String,
    pub booted: bool,
    pub evses: Vec<EVSE>,
    pub events: Vec<ScheduledEvents>,
    pub authorization: Option<AuthorizationType>, // a pending authorization that is not tied to a specific connector
}
