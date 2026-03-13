use log::info;
use ocpp_client::cp::CP;
use ocpp_client::cp_data::{
    AuthorizationType, CPData, ChargeSessionReference,
    EventTypes, ScheduledEvents, EV, EVSE, RFID,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use ocpp_client::rest_server::start_rest_server;
use reqwest;
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "INFO");
    }

    env_logger::init();

    info!("Starting CP example");
    let variables = CP::read_variables_from_file();
    CP::list_components(variables.clone());

    let cp_data = Arc::new(Mutex::new(CPData {
        username: Some("RustTest002".to_string()),
        password: Some("RustyRustRustRust".to_string()),
        supported_protocol_versions: Some("ocpp1.6, ocpp2.0.1".to_string()),
        selected_protocol: Some("ocpp1.6".to_string()),
        serial: "RustTest002".to_string(),
        model: "RustModel".to_string(),
        vendor: "RustVendor".to_string(),
        booted: false,
        evses: HashMap::new(),
        variables: variables.clone(),
        base_url: "ws://127.0.0.1:8180/steve/websocket/CentralSystemService/".to_string(),
        events: vec![
            ScheduledEvents {
                duration: 2,
                event: EventTypes::Authorize(AuthorizationType::RFID(RFID {
                    id_tag: "TAG123".to_string(),
                })),
            },
            ScheduledEvents {
                duration: 2,
                event: EventTypes::CommunicationStop,
            },
            ScheduledEvents {
                duration: 10,
                event: EventTypes::CommunicationStart,
            },
            ScheduledEvents {
                duration: 2,
                event: EventTypes::Authorize(AuthorizationType::RFID(RFID {
                    id_tag: "TAG123".to_string(),
                })),
            },
            ScheduledEvents {
                duration: 2,
                event: EventTypes::Plug(ChargeSessionReference {
                    evse_index: 1,
                    connector_id: 1,
                }),
            },
            ScheduledEvents {
                duration: 100,
                event: EventTypes::Unplug(ChargeSessionReference {
                    evse_index: 1,
                    connector_id: 1,
                }),
            },
        ],
        authorization: None,
    }));

    let ev: EV = EV {
        power_vs_soc: vec![(0.0, 50.0), (80.0, 30.0), (100.0, 0.0)],
    };

    let _ = ev;  // Unused variable, keeping for future use

    for component in variables.components {
        if component.name == "EVSE" {
            let evse: EVSE = EVSE {
                is_ac: false,
                connector_ids: vec![],
                plugged_in: None,
                started: false,
            };
            cp_data
                .lock()
                .await
                .evses
                .insert(component.evse.unwrap(), evse);
        }
        if component.name == "Connector" {
            if let Some(evse_index) = component.evse {
                if let Some(connector_id) = component.connector {
                    cp_data
                        .lock()
                        .await
                        .evses
                        .get_mut(&evse_index)
                        .unwrap()
                        .connector_ids
                        .push(connector_id as u32);
                }
            }
        }
    }

    let mut _cp1 = CP {
        communicator: Arc::new(Mutex::new(None)),
        client: None,
        data: cp_data,
    };
    //"wss://ocpp.coreevi.com/ocpp2.0.1/65/RustTest002"

    // Start REST API server in a background task
    let rest_server = start_rest_server().await;

    // Spawn REST client task to interact with the API
    let rest_client_task = tokio::spawn(async {
        rest_client_example().await
    });

    // Run CP in main task while REST server runs in background

    //let cp_task = _cp1.run().await;

    info!("CP and REST server are running. Press Ctrl-C to exit.");
    tokio::signal::ctrl_c().await?;
    // Handle Ctrl-C gracefully but let the servers continue running
   

    Ok(())
}

async fn rest_client_example() -> Result<(), String> {
    println!("\n\n=== REST Client Example - Creating CP through REST API ===\n");
    
    // Wait a moment for the server to start
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    let client = reqwest::Client::new();
    let base_url = "http://127.0.0.1:3000";
    
    // Step 1: Create a CP with serial "RustTest002"
    println!("Step 1: Creating CP with serial 'RustTest002'...");
    let cp_request = json!({
        "username": "RustTest002",
        "password": "RustyRustRustRust",
        "vendor": "RustVendor",
        "model": "RustModel",
        "serial": "RustTest002",
        "protocol": "ocpp1.6"
    });
    
    let cp_response = client
        .post(&format!("{}/cp", base_url))
        .json(&cp_request)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    
    let cp_data: Value = cp_response.json().await.map_err(|e| e.to_string())?;
    println!("Response: {}\n", serde_json::to_string_pretty(&cp_data).map_err(|e| e.to_string())?);
    
    let cp_id = cp_data["cp_id"].as_u64().ok_or("Missing cp_id in response")?;
    println!("Created CP with ID: {}\n", cp_id);
    
    // Step 2: Add multiple events to the CP using bulk endpoint
    println!("Step 2: Adding 6 events to CP using bulk endpoint...");
    
    let events_request = json!({
        "events": [
            {
                "duration": 2,
                "event_type": "authorize",
                "id_tag": "TAG123"
            },
            {
                "duration": 2,
                "event_type": "communication_stop"
            },
            {
                "duration": 10,
                "event_type": "communication_start"
            },
            {
                "duration": 2,
                "event_type": "authorize",
                "id_tag": "TAG123"
            },
            {
                "duration": 2,
                "event_type": "plug",
                "evse_index": 1,
                "connector_id": 1
            },
            {
                "duration": 100,
                "event_type": "unplug",
                "evse_index": 1,
                "connector_id": 1
            }
        ]
    });
    
    let bulk_response = client
        .post(&format!("{}/cp/{}/events/bulk", base_url, cp_id))
        .json(&events_request)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    
    let bulk_data: Value = bulk_response.json().await.map_err(|e| e.to_string())?;
    println!("Add bulk events response: {}\n", serde_json::to_string_pretty(&bulk_data).map_err(|e| e.to_string())?);
    
    // Step 3: Verify the CP was created and events were added
    println!("Step 3: Verifying CP creation using GET API...");
    
    let verify_response = client
        .get(&format!("{}/cp/{}/data", base_url, cp_id))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    
    let verified_data: Value = verify_response.json().await.map_err(|e| e.to_string())?;
    println!("Verified CP data: {}\n", serde_json::to_string_pretty(&verified_data).map_err(|e| e.to_string())?);
    
    // Check if the CP is the one we created
    let serial = verified_data["serial"]
        .as_str()
        .ok_or("Missing serial in verified data")?;
    let events_count = verified_data["events_count"]
        .as_u64()
        .ok_or("Missing events_count in verified data")?;
    
    if serial == "RustTest002" && events_count == 6 {
        println!("✓ SUCCESS: CP with serial '{}' was properly created with {} events!", serial, events_count);
    } else {
        println!("✗ MISMATCH: Expected serial 'RustTest002' with 6 events, got serial '{}' with {} events", 
                 serial, events_count);
    }
    
    // Step 4: List all CPs
    println!("\nStep 4: Listing all CPs...");
    let list_response = client
        .get(&format!("{}/cp", base_url))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    
    let all_cps: Value = list_response.json().await.map_err(|e| e.to_string())?;
    println!("All CPs: {}\n", serde_json::to_string_pretty(&all_cps).map_err(|e| e.to_string())?);
    
    Ok(())
}
