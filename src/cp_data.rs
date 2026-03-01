
#[derive(Clone)]
pub struct RFID {
    pub id_tag: String,
}

#[derive(Clone)]
pub struct PlugAndCharge {}

#[derive(Clone)]
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

#[derive(Clone, Debug, PartialEq)]
pub enum UserTransition {
    LocalStart = 0,
    LocalStop,
    Plug,
    Unplug,
    RemoteStart,
    RemoteStop,
}
#[derive(Clone)]

pub struct UserStateTransitions {
    pub duration: u32,

    pub transition: UserTransition,
}

#[derive(Clone)]

pub struct TransitionGraph {
    pub transitions: Vec<UserStateTransitions>,
}
#[derive(Clone)]

pub struct ChargeSession {
    pub ev: EV,
    pub authorization: AuthorizationType,
    pub started: bool,
    pub authorized: bool,
    pub plugged_in: bool,
    pub user_transitions: TransitionGraph,
}

#[derive(Clone)]
pub struct ChargeSessionReference {
    pub evse_index: usize,
    pub charge_session_index: usize,
}

#[derive(Clone)]
pub enum MessageReference {
    ChargeSession(ChargeSessionReference),
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
}
