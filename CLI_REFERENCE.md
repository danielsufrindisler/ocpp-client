# OCPP Client - CLI Reference Guide

This document provides complete reference for using the OCPP Client Simulator in all three modes: server-only, static testing, and interactive.

## Quick Start

Show help:
```bash
ocpp-client --help
```

Start server (interactive mode via REST API):
```bash
ocpp-client --server
```

Run comprehensive test with all 4 CP types:
```bash
cargo run --example connect -- --server --client --test data/test_all_cps.json --test_length 60
```

Run static test:
```bash
ocpp-client --client --test test_0_1.json
```

---

## Usage Modes

### Mode 1: Server Only (`--server`)

Starts a REST API server for dynamic interaction. Chargers and events are created via HTTP requests.

```bash
ocpp-client --server
```

**Best for:** 
- Web UI development
- AI agents making dynamic requests
- Manual testing via curl/Postman

**Output:**
```
REST Server started at http://127.0.0.1:3000
```

**API is live and ready to accept:**
- `POST /cp` - Create new charger
- `GET /status` - Get system status
- `POST /cp/{id}/events` - Add event to charger


### Mode 2: Static (`--client --test <FILE>`)

Loads a JSON test file, runs all chargers and events, then exits. No REST server.

```bash
ocpp-client --client --test test_0_1.json
```

**Best for:**
- CI/CD pipelines
- Reproducible test scenarios
- AI generating test files

**Flow:**
1. Load test file
2. Parse JSON configuration
3. Create chargers
4. Process all events in order
5. Exit when complete

**Exit status:** 0 on success, 1 on failure


### Mode 3: Interactive (`--server --client`)

Starts both REST API server AND allows CLI interaction. Run chargers dynamically.

```bash
ocpp-client --server --client
```

**Best for:**
- Humans exploring the system
- Mixed automated + manual testing
- Web UI + API simultaneously

**Features:**
- REST API running at http://127.0.0.1:3000
- Chargers can be added anytime via API
- Press Ctrl-C to cleanly shutdown


---

## CLI Options

### `--server`
Start the REST API server. Creates HTTP endpoint at `http://127.0.0.1:3000`.

**Can be combined with:** `--client`, `--speed`
**Cannot be combined with:** (none - always compatible)

### `--client`
Enable client mode. Loads test configurations and creates chargers via API calls.

**Can be combined with:** `--test`, `--server`, `--speed`
**Cannot be combined with:** (none - always compatible except `--test` requires `--client`)

### `--test <FILE>`
Load test configuration from JSON file. **Requires `--client`.**

**Argument:** Relative or absolute path to JSON file
**Required when:** Running static tests
**Example:** `--test data/test_0_1.json`

### `--speed <MULTIPLIER>`
Simulation speed multiplier. Controls how fast time flows in the simulator.

**Default:** 1.0 (real-time)
**Examples:**
- `--speed 2.0` - Run 2x faster (events happen at 50% time)
- `--speed 0.5` - Run 50% slower (events happen at 200% time)
- `--speed 100` - Run 100x faster (for fast testing)

**Constraint:** Must be positive number > 0

**Can be combined with:** `--server`, `--client`, `--test`

### `--help`, `-h`
Display help message and exit.

---

## Test File Format

Static tests use JSON configuration files. The format is:

```json
{
  "cps": [
    "charger_1",
    "charger_2"
  ],
  "events": [
    {
      "charger_id": "charger_1",
      "event_type": "plug",
      "duration": 5,
      "evse_index": 1,
      "connector_id": 1,
      "ev_power": 7.4,
      "ev_capacity": 60.0,
      "ev_soc": 15.0,
      "ev_final_soc": 85.0
    }
  ]
}
```

### JSON Structure

#### `cps` (array, required)
List of charger configuration names to load. These load from `data/device_model_<name>.json` files.

```json
"cps": ["charger_1", "charger_2"]
```

Each charger file should be a valid OCPP 2.0.1 device model JSON.

#### `events` (array, optional)
List of events to run in sequence. Events run in order specified.

```json
"events": [
  { "event_type": "authorize", "duration": 0, "id_tag": "TAG_12345" },
  { "event_type": "plug", "duration": 5, "evse_index": 1, "connector_id": 1, "ev_power": 7.4, "ev_capacity": 60.0, "ev_soc": 20.0, "ev_final_soc": 80.0 }
]
```

### Event Properties

#### Common
- **duration** (u32, required): Seconds from NOW to trigger event (0 = immediate)
- **event_type** (string, required): Type of event (see below)

#### By Event Type

##### `authorize`
Trigger RFID authorization.

```json
{
  "event_type": "authorize",
  "duration": 0,
  "id_tag": "TAG_12345"
}
```

**Parameters:**
- `duration` (u32): Delay before authorization
- `id_tag` (string, optional): RFID tag ID. Default: "DEFAULT_TAG"

##### `plug`
Connect EV to charger.

```json
{
  "event_type": "plug",
  "duration": 5,
  "evse_index": 1,
  "connector_id": 1,
  "ev_power": 7.4,
  "ev_capacity": 60.0,
  "ev_soc": 20.0,
  "ev_final_soc": 80.0
}
```

**Parameters:**
- `duration` (u32): Delay before plug
- `evse_index` (u32, required): EVSE ID  
- `connector_id` (u32, required): Connector ID on EVSE
- `ev_power` (f32, optional): Charging power in kW. Default: 7.0
- `ev_capacity` (f32, optional): EV battery capacity in kWh. Default: 50.0
- `ev_soc` (f32, optional): Starting SOC 0-100%. Default: 20.0
- `ev_final_soc` (f32, optional): Target SOC 0-100%. Default: 80.0

##### `unplug`
Disconnect EV from charger.

```json
{
  "event_type": "unplug",
  "duration": 600,
  "evse_index": 1,
  "connector_id": 1
}
```

**Parameters:**
- `duration` (u32): Delay before unplug
- `evse_index` (u32, required): EVSE ID
- `connector_id` (u32, required): Connector ID

##### `remote_start`
Server-initiated charging start.

```json
{
  "event_type": "remote_start",
  "duration": 10,
  "evse_index": 1,
  "connector_id": 1
}
```

**Parameters:**
- `duration` (u32): Delay before remote start
- `evse_index` (u32, required): EVSE ID
- `connector_id` (u32, required): Connector ID

##### `remote_stop`
Server-initiated charging stop.

```json
{
  "event_type": "remote_stop",
  "duration": 300,
  "evse_index": 1,
  "connector_id": 1
}
```

##### `local_stop`
Local stop button pressed.

```json
{
  "event_type": "local_stop",
  "duration": 300
}
```

**Parameters:**
- `duration` (u32): Delay before local stop

##### `communication_start`
Simulate communication restoration.

```json
{
  "event_type": "communication_start",
  "duration": 60
}
```

##### `communication_stop`
Simulate communication loss.

```json
{
  "event_type": "communication_stop",
  "duration": 30
}
```

---

## REST API Reference

### Base URL
```
http://127.0.0.1:3000
```

### Endpoints

#### Create Charger
```
POST /cp
Content-Type: application/json

{
  "name": "charger_1",
  "use_original": false
}
```

**Response (201):**
```json
{
  "cp_id": 0,
  "message": "Charge point created successfully"
}
```

**Parameters:**
- `name` (string, required): Device model name (loads from `data/device_model_<name>.json`)
- `use_original` (boolean, optional): Use original config. Default: false

---

#### Get System Status
```
GET /status
```

**Response (200):**
```json
{
  "chargers": [
    {
      "id": 0,
      "name": "charger_1",
      "serial": "ABC123",
      "status": "Charging"
    }
  ]
}
```

---

#### Add Event to Charger
```
POST /cp/{id}/events
Content-Type: application/json

{
  "event_type": "plug",
  "duration": 5,
  "evse_index": 1,
  "connector_id": 1,
  "ev_power": 7.4,
  "ev_capacity": 60.0,
  "ev_soc": 20.0,
  "ev_final_soc": 85.0
}
```

**Response (200):**
```json
{
  "message": "Event queued successfully",
  "queue_size": 5
}
```

**Parameters:**
- Same as JSON event format above
- `duration` defaults to 0 if omitted (immediate)

---

## Usage Examples

### Example 1: Start Server

```bash
$ ocpp-client --server

[2024-03-27 10:15:32] INFO: OCPP Client starting...
[2024-03-27 10:15:32] INFO: REST Server started at http://127.0.0.1:3000
[2024-03-27 10:15:32] INFO: Interactive mode started. Send requests to http://127.0.0.1:3000
[2024-03-27 10:15:32] INFO: Press Ctrl-C to exit.
```

Then in another terminal:

```bash
$ curl -X POST http://127.0.0.1:3000/cp \
  -H "Content-Type: application/json" \
  -d '{"name":"charger_1","use_original":false}'

{"cp_id":0,"message":"Charge point created successfully"}
```

---

### Example 2: Static Test with Speed

```bash
$ ocpp-client --client --test data/test_0_1.json --speed 5.0

[2024-03-27 10:15:32] INFO: Static mode: Loading test configuration from data/test_0_1.json
[2024-03-27 10:15:32] INFO: Test config contains 2 chargers
[2024-03-27 10:15:32] INFO: Created charger 'charger_1' with ID: 0
[2024-03-27 10:15:32] INFO: Created charger 'charger_2' with ID: 1
[2024-03-27 10:15:32] INFO: Test configuration loaded successfully
[2024-03-27 10:15:32] INFO: Test completed, exiting.
```

---

### Example 3: Interactive with Events

```bash
$ ocpp-client --server --client --speed 2.0

[2024-03-27 10:15:32] INFO: OCPP Client starting...
[2024-03-27 10:15:32] INFO: REST Server started at http://127.0.0.1:3000
[2024-03-27 10:15:32] INFO: Interactive mode started.
```

Then make requests:

```bash
# Create charger
curl -X POST http://127.0.0.1:3000/cp \
  -H "Content-Type: application/json" \
  -d '{"name":"charger_1"}'

# Add plug event to charger 0
curl -X POST http://127.0.0.1:3000/cp/0/events \
  -H "Content-Type: application/json" \
  -d '{
    "event_type": "plug",
    "duration": 0,
    "evse_index": 1,
    "connector_id": 1,
    "ev_power": 7.4,
    "ev_capacity": 60.0,
    "ev_soc": 20.0,
    "ev_final_soc": 80.0
  }'

# Add authorize event
curl -X POST http://127.0.0.1:3000/cp/0/events \
  -H "Content-Type: application/json" \
  -d '{
    "event_type": "authorize",
    "duration": 2,
    "id_tag": "TAG_12345"
  }'
```

---

## Configuration Files

### Device Model Files

Located in `data/` directory. Format: `device_model_<name>.json`

Examples:
- `data/device_model_charger_1.json`
- `data/device_model.json` (default)
- `data/device_model_ocpp16.json` (OCPP 1.6 variant)

---

## Troubleshooting

### "test config must contain cps array"
Your JSON file is missing the `"cps"` array. Add it:
```json
{
  "cps": ["charger_1"],
  "events": []
}
```

### "--test can only be used with --client mode"
You tried to specify `--test` without `--client`. Use:
```bash
ocpp-client --client --test <file>
```

### Connection refused on http://127.0.0.1:3000
The REST server isn't running. Make sure you used `--server` flag:
```bash
ocpp-client --server
```

### Events not being processed
1. Check that `--client` is enabled (if using `--server` alone, no events process from files)
2. Verify event `duration` - events happen `duration` seconds from NOW
3. Check logs for any parsing errors in the JSON

---

## Comprehensive Testing with test_all_cps.json

The `data/test_all_cps.json` file provides a complete test configuration with 4 different CP types for comprehensive testing.

### Running the Test

```bash
cargo run --example connect -- --server --client --test data/test_all_cps.json --test_length 60
```

Or use the automated Python script:

```bash
python3 test_automation.py
```

### The 4 CP Types

#### CP_1_Full
**Purpose:** Most complete CP implementation with all OCPP features
**Features:**
- Full configuration (username, password, vendor, model, serial)
- Event capabilities (authorize, plug, unplug, fault)
- Variable configurations
- EVSE and connector configurations
- Component definitions for device model

**Use Case:** Full functionality testing, comprehensive message validation

#### CP_2_NoEvents
**Purpose:** Minimal CP for testing core functionality
**Features:**
- Basic properties only
- No event capabilities (no "events" array)
- Simple configuration

**Use Case:** Testing server stability with inactive CPs, basic connectivity

#### CP_3_ComponentsOnly
**Purpose:** CP focused on component-based device model testing
**Features:**
- Basic properties
- Minimal events for testing
- Component and variable focus

**Use Case:** Device model validation, component testing

#### CP_4_RESTOnly
**Purpose:** CP created exclusively via REST API at runtime
**Features:**
- Not pre-defined in test configuration
- Created dynamically via `/cp/from-rest` endpoint
- Demonstrates REST API capabilities

**Use Case:** REST API testing, dynamic CP creation validation

### Python Test Automation

The `test_automation.py` script provides automated testing of all 4 CP types:

```python
python3 test_automation.py
```

**Script Actions:**
1. Starts OCPP server with `test_all_cps.json`
2. Waits for server initialization
3. Creates CP_4 via REST API
4. Triggers plug and authorize events on all CPs
5. Retrieves message statistics
6. Displays results for each CP

**Statistics Output:**
```
=== MESSAGE STATISTICS ===
{
  "cps": {
    "0": {
      "cp_serial": "CP_1_Full",
      "messages": {
        "Authorize": {"sent_count": ..., "sent_success": ..., ...},
        ...
      },
      "total_energy_delivered_kwh": ...
    },
    ...
  }
}
```

---

## See Also

- [REST_API.md](REST_API.md) - Detailed REST API documentation
- [README.md](README.md) - Project overview
- [CONTRIBUTING.md](CONTRIBUTING.md) - Development guidelines

