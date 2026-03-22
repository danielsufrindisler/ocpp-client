use log::info;
use ocpp_client::rest_server::start_rest_server;
use reqwest;
use serde_json::{json, Value};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "INFO");
    }

    env_logger::init();

    // Start REST API server in a background task
    let _rest_server = start_rest_server().await;

    // Spawn REST client task to interact with the API
    let _rest_client_task = tokio::spawn(async { rest_client_example().await });


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
    println!(
        "Response: {}\n",
        serde_json::to_string_pretty(&cp_data).map_err(|e| e.to_string())?
    );

    let cp_id = cp_data["cp_id"]
        .as_u64()
        .ok_or("Missing cp_id in response")?;
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
                "connector_id": 1,
                "power_vs_soc": [[0.0, 50.0], [20.0, 105.0],[80.0, 105.0], [100.0, 2.0]],
                "soc": 20.0,
                "final_soc": 80.0,
                "capacity": 88.0
            },
            {
                "duration": 60,
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
    println!(
        "Add bulk events response: {}\n",
        serde_json::to_string_pretty(&bulk_data).map_err(|e| e.to_string())?
    );

    // Step 3: Verify the CP was created and events were added
    println!("Step 3: Verifying CP creation using GET API...");

    let verify_response = client
        .get(&format!("{}/cp/{}/data", base_url, cp_id))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let verified_data: Value = verify_response.json().await.map_err(|e| e.to_string())?;
    println!(
        "Verified CP data: {}\n",
        serde_json::to_string_pretty(&verified_data).map_err(|e| e.to_string())?
    );

    // Check if the CP is the one we created
    let serial = verified_data["serial"]
        .as_str()
        .ok_or("Missing serial in verified data")?;
    let events_count = verified_data["events_count"]
        .as_u64()
        .ok_or("Missing events_count in verified data")?;

    if serial == "RustTest002" && events_count == 6 {
        println!(
            "✓ SUCCESS: CP with serial '{}' was properly created with {} events!",
            serial, events_count
        );
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
    println!(
        "All CPs: {}\n",
        serde_json::to_string_pretty(&all_cps).map_err(|e| e.to_string())?
    );

    Ok(())
}
