# OCPP CLI Quick Reference Guide

## Overview
The CLI module provides a JSON-based interface for creating and managing virtual charging stations. This is designed for AI agents to control OCPP charger simulations.

## Getting Help

```rust
// Display comprehensive help text
let help_text = ocpp_client::cli::cli_help();
println!("{}", help_text);
```

## Command Examples

### 1. Create a Charger (EVSE)

Create an AC charger with EVSE ID 1 and connector 1:

```json
{
  "command": "CREATE_CHARGER",
  "evse_id": 1,
  "connector_id": 1,
  "is_ac": true
}
```

Create a DC charger:

```json
{
  "command": "CREATE_CHARGER",
  "evse_id": 2,
  "connector_id": 1,
  "is_ac": false
}
```

### 2. Create Events

#### Authorization Event (Immediate)

```json
{
  "command": "CREATE_EVENT",
  "event_type": "authorize",
  "duration": 0,
  "id_tag": "TAG_12345"
}
```

#### Plug EV (Delayed by 5 seconds)

```json
{
  "command": "CREATE_EVENT",
  "event_type": "plug",
  "duration": 5,
  "evse_index": 1,
  "connector_id": 1,
  "ev_power": 7.4,
  "ev_capacity": 50.0,
  "ev_soc": 20.0,
  "ev_final_soc": 80.0
}
```

#### Remote Start Transaction (Server-initiated)

```json
{
  "command": "CREATE_EVENT",
  "event_type": "remote_start",
  "duration": 10,
  "evse_index": 1,
  "connector_id": 1
}
```

#### Stop Charging (Local)

```json
{
  "command": "CREATE_EVENT",
  "event_type": "local_stop",
  "duration": 300
}
```

#### Unplug EV

```json
{
  "command": "CREATE_EVENT",
  "event_type": "unplug",
  "duration": 600,
  "evse_index": 1,
  "connector_id": 1
}
```

### 3. Query Status

List all chargers:

```json
{
  "command": "LIST_CHARGERS"
}
```

List all scheduled events:

```json
{
  "command": "LIST_EVENTS"
}
```

Get system status:

```json
{
  "command": "GET_STATUS"
}
```

## Usage in Code

### Basic Example

```rust
use ocpp_client::cli::{parse_command, build_event_from_command, cli_help};

// Get help for user documentation
let help = cli_help();
println!("Available commands:\n{}", help);

// Parse a command from JSON string
let json = r#"{"command": "CREATE_CHARGER", "evse_id": 1, "connector_id": 1, "is_ac": true}"#;
let cmd = parse_command(json).expect("Invalid JSON");

// For CREATE_EVENT, convert to ScheduledEvents
let json = r#"{"command": "CREATE_EVENT", "event_type": "authorize", "duration": 2, "id_tag": "TAG123"}"#;
let cmd = parse_command(json).expect("Invalid JSON");
if let Ok(event) = build_event_from_command(&cmd) {
    // Use the event in your charger simulation
    println!("Scheduled event: {:?}", event);
}
```

### With Response Handling

```rust
use ocpp_client::cli::CliResponse;
use serde_json::json;

// Simulate processing a command
match parse_command(command_json) {
    Ok(cmd) => {
        match cmd {
            CliCommand::CreateCharger { evse_id, connector_id, is_ac } => {
                // Process charger creation
                let response = CliResponse::success(
                    "Charger created successfully",
                    Some(json!({
                        "evse_id": evse_id,
                        "connector_id": connector_id,
                        "is_ac": is_ac
                    }))
                );
                println!("{}", serde_json::to_string_pretty(&response).unwrap());
            }
            _ => {
                // Handle other commands
            }
        }
    }
    Err(e) => {
        let response = CliResponse::error(&e, Some("INVALID_JSON"));
        println!("{}", serde_json::to_string_pretty(&response).unwrap());
    }
}
```

## Event Types Reference

| Type | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `authorize` | RFID authorization | duration | id_tag |
| `plug` | EV connects to charger | duration, evse_index, connector_id | ev_power, ev_capacity, ev_soc, ev_final_soc |
| `unplug` | EV disconnects | duration, evse_index, connector_id | |
| `remote_start` | Server starts charging | duration, evse_index, connector_id | |
| `remote_stop` | Server stops charging | duration, evse_index, connector_id | |
| `local_stop` | Local stop command | duration | |

## Parameter Reference

### Charger Parameters
- `evse_id` (u32): Unique EVSE identifier (1-N)
- `connector_id` (u32): Connector number per EVSE (typically 1-4)
- `is_ac` (bool): true for AC charging, false for DC

### Event Parameters
- `duration` (u32): Seconds until event triggers (0 = immediate)
- `evse_index` (u32): Which EVSE to use
- `connector_id` (u32): Which connector on the EVSE
- `id_tag` (string): RFID tag identifier (default: "DEFAULT_TAG")
- `ev_power` (f32): Charging power in kW (default: 7.0)
- `ev_capacity` (f32): Battery capacity in kWh (default: 50.0)
- `ev_soc` (f32): Initial State of Charge 0-100% (default: 20.0)
- `ev_final_soc` (f32): Target State of Charge 0-100% (default: 80.0)

## AI Agent Integration

### 1. Initialization
```rust
// Get help documentation
let help = ocpp_client::cli::cli_help();
// Parse to understand available commands
```

### 2. Scenario Creation
```rust
// Create multiple chargers
for i in 1..=4 {
    let cmd = format!(r#"{{"command": "CREATE_CHARGER", "evse_id": {}, "connector_id": 1, "is_ac": true}}"#, i);
    parse_command(&cmd).ok();
}

// Schedule events
let events = vec![
    r#"{"command": "CREATE_EVENT", "event_type": "authorize", "duration": 2, "id_tag": "TAG001"}"#,
    r#"{"command": "CREATE_EVENT", "event_type": "plug", "duration": 5, "evse_index": 1, "connector_id": 1}"#,
    r#"{"command": "CREATE_EVENT", "event_type": "remote_start", "duration": 10, "evse_index": 1, "connector_id": 1}"#,
];
```

### 3. Monitoring
```rust
// Check status
let status_cmd = r#"{"command": "GET_STATUS"}"#;
parse_command(status_cmd).ok();

// List all events
let list_cmd = r#"{"command": "LIST_EVENTS"}"#;
parse_command(list_cmd).ok();
```

## Protocol Support

The CLI works with both OCPP versions:

- **OCPP 1.6**: StartTransaction, StopTransaction, MeterValues
- **OCPP 2.0.1**: TransactionEvent, MeterValues, GetVariables

The charger automatically handles the selected protocol version.

## Error Handling

All commands return consistent error responses:

```json
{
  "success": false,
  "message": "Event creation failed: missing required parameter 'evse_index'",
  "error_code": "MISSING_PARAMETER"
}
```

Check the `success` field to determine if the command executed successfully.

## Performance Notes

- Commands are processed synchronously
- Event scheduling is non-blocking
- Multiple chargers can be created and managed simultaneously
- CLI parsing has minimal overhead (~1-2µs per command)

## Testing

Run the CLI unit tests:

```bash
cargo test --lib cli
```

Tests validate:
- JSON parsing
- Event creation
- Help text generation
