use crate::cp_data::{
    AuthorizationType, ChargeSessionReference, EventTypes, ScheduledEvents, EV, RFID,
};
use serde::{Deserialize, Serialize};

/// CLI command for creating a charger/EVSE
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChargerCommand {
    pub evse_id: u32,
    pub connector_id: u32,
    pub is_ac: bool,
}

/// CLI command for creating an EV event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEventCommand {
    pub event_type: String, // "authorize", "plug", "unplug", "remote_start", "remote_stop", "local_stop"
    pub duration: u32,
    pub evse_index: Option<u32>,
    pub connector_id: Option<u32>,
    pub id_tag: Option<String>,
    pub ev_power: Option<f32>,
    pub ev_capacity: Option<f32>,
    pub ev_soc: Option<f32>,
    pub ev_final_soc: Option<f32>,
}

/// Help text for CLI interface - designed for AI agent parsing
pub fn cli_help() -> String {
    r#"
OCPP Charger Simulator - CLI Interface
======================================

This CLI allows creating and managing virtual charging stations for testing OCPP protocols.

COMMANDS:

1. CREATE_CHARGER
   Description: Create a new charging station/EVSE
   Required Parameters:
     - evse_id: u32 (unique identifier for EVSE, e.g., 1, 2, 3)
     - connector_id: u32 (connector ID within EVSE, usually 1-4)
     - is_ac: bool (true for AC charging, false for DC)
   
   Example JSON:
   {
     "command": "CREATE_CHARGER",
     "evse_id": 1,
     "connector_id": 1,
     "is_ac": true
   }

2. CREATE_EVENT
   Description: Create a scheduled event for testing (authorize, plug, charge, unplug, etc.)
   Required Parameters:
     - event_type: string - One of:
         * "authorize" - Trigger RFID authorization
         * "plug" - Connect EV to charger
         * "unplug" - Disconnect EV from charger
         * "remote_start" - Start charging from server
         * "remote_stop" - Stop charging from server
         * "local_stop" - Local stop request
     - duration: u32 (seconds until event occurs, 0 for immediate)
   
   Optional Parameters (for "plug" and "authorize" events):
     - evse_index: u32 (EVSE to use, required for "plug")
     - connector_id: u32 (connector to use, required for "plug")
     - id_tag: string (RFID tag ID for "authorize" events, default: "DEFAULT_TAG")
     - ev_power: f32 (EV charging power in kW, default: 7.0)
     - ev_capacity: f32 (EV battery capacity in kWh, default: 50.0)
     - ev_soc: f32 (starting State of Charge 0-100%, default: 20.0)
     - ev_final_soc: f32 (target State of Charge 0-100%, default: 80.0)
   
   Example JSON - Authorize:
   {
     "command": "CREATE_EVENT",
     "event_type": "authorize",
     "duration": 2,
     "id_tag": "MYTAG123"
   }
   
   Example JSON - Plug EV:
   {
     "command": "CREATE_EVENT",
     "event_type": "plug",
     "duration": 5,
     "evse_index": 1,
     "connector_id": 1,
     "ev_power": 7.4,
     "ev_capacity": 60.0,
     "ev_soc": 15.0,
     "ev_final_soc": 85.0
   }
   
   Example JSON - Remote Start:
   {
     "command": "CREATE_EVENT",
     "event_type": "remote_start",
     "duration": 10,
     "evse_index": 1,
     "connector_id": 1
   }

3. LIST_CHARGERS
   Description: List all configured chargers/EVSEs
   Example JSON:
   {
     "command": "LIST_CHARGERS"
   }

4. LIST_EVENTS
   Description: List all scheduled events
   Example JSON:
   {
     "command": "LIST_EVENTS"
   }

5. GET_STATUS
   Description: Get current status of charger
   Example JSON:
   {
     "command": "GET_STATUS"
   }

RESPONSE FORMAT:
All commands return JSON responses with the following structure:
{
  "success": bool,
  "message": string,
  "data": object (optional, command-specific)
}

ERROR HANDLING:
If a command fails, the response will include:
{
  "success": false,
  "message": "Error description",
  "error_code": string (optional)
}

PROTOCOL SUPPORT:
- OCPP 1.6 (StartTransaction, StopTransaction, MeterValues, etc.)
- OCPP 2.0.1 (TransactionEvent, MeterValues, GetVariables, etc.)
"#
    .to_string()
}

/// Parse CLI command from JSON string
pub fn parse_command(json_str: &str) -> Result<CliCommand, String> {
    match serde_json::from_str::<serde_json::Value>(json_str) {
        Ok(json) => match json.get("command").and_then(|c| c.as_str()) {
            Some("CREATE_CHARGER") => {
                let evse_id = json
                    .get("evse_id")
                    .and_then(|v| v.as_u64())
                    .ok_or("Missing or invalid evse_id")? as u32;
                let connector_id =
                    json.get("connector_id")
                        .and_then(|v| v.as_u64())
                        .ok_or("Missing or invalid connector_id")? as u32;
                let is_ac = json
                    .get("is_ac")
                    .and_then(|v| v.as_bool())
                    .ok_or("Missing or invalid is_ac")?;

                Ok(CliCommand::CreateCharger {
                    evse_id,
                    connector_id,
                    is_ac,
                })
            }
            Some("CREATE_EVENT") => {
                let event_type = json
                    .get("event_type")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing or invalid event_type")?
                    .to_string();
                let duration = json
                    .get("duration")
                    .and_then(|v| v.as_u64())
                    .ok_or("Missing or invalid duration")? as u32;
                let evse_index = json
                    .get("evse_index")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                let connector_id = json
                    .get("connector_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                let id_tag = json
                    .get("id_tag")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let ev_power = json
                    .get("ev_power")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32);
                let ev_capacity = json
                    .get("ev_capacity")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32);
                let ev_soc = json
                    .get("ev_soc")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32);
                let ev_final_soc = json
                    .get("ev_final_soc")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32);

                Ok(CliCommand::CreateEvent {
                    event_type,
                    duration,
                    evse_index,
                    connector_id,
                    id_tag,
                    ev_power,
                    ev_capacity,
                    ev_soc,
                    ev_final_soc,
                })
            }
            Some("LIST_CHARGERS") => Ok(CliCommand::ListChargers),
            Some("LIST_EVENTS") => Ok(CliCommand::ListEvents),
            Some("GET_STATUS") => Ok(CliCommand::GetStatus),
            Some("HELP") => Ok(CliCommand::Help),
            Some(cmd) => Err(format!("Unknown command: {}", cmd)),
            None => Err("Missing 'command' field".to_string()),
        },
        Err(e) => Err(format!("Invalid JSON: {}", e)),
    }
}

/// CLI commands
#[derive(Debug, Clone)]
pub enum CliCommand {
    CreateCharger {
        evse_id: u32,
        connector_id: u32,
        is_ac: bool,
    },
    CreateEvent {
        event_type: String,
        duration: u32,
        evse_index: Option<u32>,
        connector_id: Option<u32>,
        id_tag: Option<String>,
        ev_power: Option<f32>,
        ev_capacity: Option<f32>,
        ev_soc: Option<f32>,
        ev_final_soc: Option<f32>,
    },
    ListChargers,
    ListEvents,
    GetStatus,
    Help,
}

/// Build event from CREATE_EVENT command
pub fn build_event_from_command(cmd: &CliCommand) -> Result<ScheduledEvents, String> {
    match cmd {
        CliCommand::CreateEvent {
            event_type,
            duration,
            evse_index,
            connector_id,
            id_tag,
            ev_power,
            ev_capacity,
            ev_soc,
            ev_final_soc,
        } => {
            let event = match event_type.as_str() {
                "authorize" => {
                    let tag = id_tag.clone().unwrap_or_else(|| "DEFAULT_TAG".to_string());
                    EventTypes::Authorize(AuthorizationType::RFID(RFID { id_tag: tag }))
                }
                "plug" => {
                    let evse_idx = evse_index.ok_or("evse_index required for plug event")?;
                    let conn_id = connector_id.ok_or("connector_id required for plug event")?;

                    let ev = EV {
                        power_vs_soc: vec![(0.0, ev_power.unwrap_or(7.0)), (100.0, 0.0)],
                        soc: ev_soc.unwrap_or(20.0),
                        final_soc: ev_final_soc.unwrap_or(80.0),
                        capacity: ev_capacity.unwrap_or(50.0),
                        power: ev_power.unwrap_or(7.0),
                    };

                    EventTypes::Plug(
                        ChargeSessionReference {
                            evse_index: evse_idx,
                            connector_id: conn_id,
                        },
                        ev,
                    )
                }
                "unplug" => {
                    let evse_idx = evse_index.ok_or("evse_index required for unplug event")?;
                    let conn_id = connector_id.ok_or("connector_id required for unplug event")?;
                    EventTypes::Unplug(ChargeSessionReference {
                        evse_index: evse_idx,
                        connector_id: conn_id,
                    })
                }
                "remote_start" => {
                    let evse_idx = evse_index.ok_or("evse_index required for remote_start event")?;
                    let conn_id = connector_id.ok_or("connector_id required for remote_start event")?;
                    EventTypes::RemoteStart(ChargeSessionReference {
                        evse_index: evse_idx,
                        connector_id: conn_id,
                    })
                }
                "remote_stop" => {
                    let evse_idx = evse_index.ok_or("evse_index required for remote_stop event")?;
                    let conn_id = connector_id.ok_or("connector_id required for remote_stop event")?;
                    EventTypes::RemoteStop(ChargeSessionReference {
                        evse_index: evse_idx,
                        connector_id: conn_id,
                    })
                }
                "local_stop" => EventTypes::LocalStop,
                _ => {
                    return Err(format!(
                        "Unknown event type: {}. Valid types: authorize, plug, unplug, remote_start, remote_stop, local_stop",
                        event_type
                    ))
                }
            };

            Ok(ScheduledEvents {
                duration: *duration,
                event,
            })
        }
        _ => Err("Command is not a CREATE_EVENT".to_string()),
    }
}

/// Response structure for CLI commands
#[derive(Debug, Serialize, Deserialize)]
pub struct CliResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

impl CliResponse {
    pub fn success(message: impl Into<String>, data: Option<serde_json::Value>) -> Self {
        Self {
            success: true,
            message: message.into(),
            data,
            error_code: None,
        }
    }

    pub fn error(message: impl Into<String>, error_code: Option<&str>) -> Self {
        Self {
            success: false,
            message: message.into(),
            data: None,
            error_code: error_code.map(|s| s.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_create_charger() {
        let json =
            r#"{"command": "CREATE_CHARGER", "evse_id": 1, "connector_id": 1, "is_ac": true}"#;
        let cmd = parse_command(json).unwrap();
        match cmd {
            CliCommand::CreateCharger {
                evse_id,
                connector_id,
                is_ac,
            } => {
                assert_eq!(evse_id, 1);
                assert_eq!(connector_id, 1);
                assert_eq!(is_ac, true);
            }
            _ => panic!("Wrong command type"),
        }
    }

    #[test]
    fn test_parse_create_event() {
        let json = r#"{"command": "CREATE_EVENT", "event_type": "authorize", "duration": 2, "id_tag": "TAG123"}"#;
        let cmd = parse_command(json).unwrap();
        match cmd {
            CliCommand::CreateEvent {
                event_type,
                duration,
                ..
            } => {
                assert_eq!(event_type, "authorize");
                assert_eq!(duration, 2);
            }
            _ => panic!("Wrong command type"),
        }
    }

    #[test]
    fn test_cli_help() {
        let help = cli_help();
        assert!(help.contains("CREATE_CHARGER"));
        assert!(help.contains("CREATE_EVENT"));
        assert!(help.contains("OCPP"));
    }
}
