# OCPP Charge Point REST API

A comprehensive REST API for managing OCPP Charge Points (CPs). This API allows you to create charge points, manage their configurations, and schedule events dynamically.

## Overview

The REST API provides a simple HTTP interface to:
- Create new Charge Points with initial configuration
- Add scheduled events to charge points
- Retrieve charge point data and status
- List all created charge points

## Architecture

The API uses:
- **Framework**: Axum (async Rust web framework)
- **Storage**: In-memory with `Arc<RwLock<HashMap>>` for thread-safe access
- **Async Runtime**: Tokio
- **Serialization**: Serde JSON

## Running the Server

```bash
cargo run --example rest_api
```

The server will start on `http://127.0.0.1:3000`

## API Documentation

The REST API includes interactive Swagger/OpenAPI documentation available at:
- **Swagger UI**: `http://127.0.0.1:3000/swagger-ui`
- **OpenAPI JSON**: `http://127.0.0.1:3000/api-docs/openapi.json`

## API Endpoints

### 1. Create a Charge Point from REST

**Endpoint**: `POST /cp/from-rest`

Creates a new Charge Point with full runtime configuration supplied in a single REST call.

This endpoint is the preferred way to create a test charger when you want to define:
- the number of EVSEs
- how many connectors each EVSE exposes
- optional device model configuration variables such as `MeterValueSampleInterval`

For creating multiple CPs at once, use the bulk endpoint:
- `POST /cp/from-rest/bulk`

Each CP in the bulk request can include its own `evses` and `configuration` arrays.

**Minimum required fields**:
- `vendor` (string)
- `model` (string)
- `serial` (string)
- `protocol` (string)
- `evses` (array of EVSE definitions)

**Optional fields**:
- `username` (string) - only required for security profile 6
- `password` (string) - only required for security profile 6
- `configuration` (array) - device model configuration variables

Each EVSE definition requires:
- `evse_index` (unique integer)
- `connector_count` (number of connectors on that EVSE)

**Request Body**:
```json
{
  "vendor": "TestVendor",
  "model": "TestModel_4",
  "serial": "CP_4_RESTOnly",
  "protocol": "ocpp1.6",
  "username": "CP_4_User",
  "password": "CP_4_Pass",
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
}
```

**Response** (201 Created):
```json
{
  "cp_id": 0,
  "message": "CP 0 created successfully from REST parameters"
}
```

**Example**:
```bash
curl -X POST http://localhost:3000/cp/from-rest \
  -H "Content-Type: application/json" \
  -d '{
    "vendor": "TestVendor",
    "model": "TestModel_4",
    "serial": "CP_4_RESTOnly",
    "protocol": "ocpp1.6",
    "username": "CP_4_User",
    "password": "CP_4_Pass",
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
  }'
```

> If you want to create a charger from existing JSON files instead, use `POST /cp/from-file`.

---

### 2. List All Charge Points

**Endpoint**: `GET /cp`

Retrieves a list of all created Charge Points with their data.

**Response** (200 OK):
```json
[
  {
    "cp_id": 0,
    "username": "TestCP001",
    "password": "secure_password",
    "vendor": "TestVendor",
    "model": "TestModel",
    "serial": "SN12345",
    "protocol": "ocpp1.6",
    "booted": false,
    "events_count": 0,
    "total_energy_delivered_kwh": 0.0
  },
  ...
]
```

**Example**:
```bash
curl http://localhost:3000/cp
```

---

### 3. Get Charge Point Data

**Endpoint**: `GET /cp/{cp_id}`

Retrieves the current data for a specific Charge Point.

**Response** (200 OK):
```json
{
  "cp_id": 0,
  "username": "TestCP001",
  "password": "secure_password",
  "vendor": "TestVendor",
  "model": "TestModel",
  "serial": "SN12345",
  "protocol": "ocpp1.6",
  "booted": false,
  "events_count": 1,
  "total_energy_delivered_kwh": 0.0
}
```

**Error Response** (404 Not Found):
```json
{
  "error": "Charge point not found",
  "details": "No charge point with ID 999"
}
```

**Example**:
```bash
curl http://localhost:3000/cp/0
```

---

### 4. Add a Scheduled Event to a Charge Point

**Endpoint**: `POST /cp/{cp_id}/events`

Adds a scheduled event to a Charge Point. Events are processed sequentially in order of delay.

**Delay Semantics**: The `delay` field specifies when the event should trigger:
- `delay: 0` means "trigger now" (immediately, or queued if other events are pending)
- `delay: 5` means "trigger 5 seconds after simulation start"
- Events are processed in order; if an earlier event is still running, later events wait

**Request Body**:
```json
{
  "delay": 5,
  "event_type": "authorize",
  "id_tag": "TAG123"
}
```

**Response** (201 Created):
```json
{
  "message": "Event added successfully",
  "cp_id": 0
}
```

**Event Types** and Required Fields:

| Event Type | Required Fields | Notes |
|---|---|---|
| `authorize` | `id_tag` | Authorize the next transaction for a given RFID tag.
| `plug` | `evse_index`, `connector_id` | Use both the EVSE index and connector ID together. For example, connector 3 can be `evse_index: 1` and `connector_id: 3` if that connector exists.
| `unplug` | `evse_index`, `connector_id` | Unplug the given connector.
| `communication_start` | - | Resume communication.
| `communication_stop` | - | Stop communication.
| `local_stop` | - | Local stop charging.
| `faulted` | - | Optional `connector_ids` to fault specific connectors. Empty = all connectors.
| `fault_cleared` | - | Optional `connector_ids` to clear faults. Empty = all connectors.

**Examples**:

Authorization event:
```bash
curl -X POST http://localhost:3000/cp/0/events \
  -H "Content-Type: application/json" \
  -d '{
    "delay": 5,
    "event_type": "authorize",
    "id_tag": "TAG123"
  }'
```

Plug event on connector 3:
```bash
curl -X POST http://localhost:3000/cp/0/events \
  -H "Content-Type: application/json" \
  -d '{
    "delay": 10,
    "event_type": "plug",
    "evse_index": 1,
    "connector_id": 3
  }'
```

Communication stop event:
```bash
curl -X POST http://localhost:3000/cp/0/events \
  -H "Content-Type: application/json" \
  -d '{
    "delay": 2,
    "event_type": "communication_stop"
  }'
```

---

### 5. Add Multiple Events to Multiple Charge Points

**Endpoint**: `POST /cp/events`

Add events to one or more charge points in a single request.

**Request Body**:
```json
{
  "0": [
    {
      "delay": 5,
      "event_type": "authorize",
      "id_tag": "TAG_CP0_001"
    },
    {
      "delay": 10,
      "event_type": "plug",
      "evse_index": 1,
      "connector_id": 1
    }
  ],
  "1": [
    {
      "delay": 2,
      "event_type": "authorize",
      "id_tag": "TAG_CP1_001"
    }
  ]
}
```

**Response** (201 Created):
```json
{
  "message": "Events added successfully",
  "cps": 2,
  "total_events": 3
}
```

**Example**:
```bash
curl -X POST http://localhost:3000/cp/events \
  -H "Content-Type: application/json" \
  -d '{
    "0": [
      { "delay": 5, "event_type": "authorize", "id_tag": "TAG_001" },
      { "delay": 10, "event_type": "plug", "evse_index": 1, "connector_id": 1 }
    ],
    "1": [
      { "delay": 2, "event_type": "authorize", "id_tag": "TAG_002" }
    ]
  }'
```

---

## Complete Workflow Example

This example demonstrates creating two charge points and scheduling events for both simultaneously.

```bash
# 1. Create two CPs in a single request
RESPONSE=$(curl -s -X POST http://localhost:3000/cp/from-rest/bulk \
  -H "Content-Type: application/json" \
  -d '[
    {
      "vendor": "SieVendor",
      "model": "AC-22",
      "serial": "SIE001",
      "protocol": "ocpp1.6",
      "evses": [
        { "evse_index": 1, "connector_count": 1 }
      ],
      "configuration": [
        {
          "name": "OCPPCommCtrlr",
          "variables": [
            { "name": "MeterValueSampleInterval", "value": "10", "required": true }
          ]
        }
      ]
    },
    {
      "vendor": "TeslaVendor",
      "model": "SuperCharger",
      "serial": "TSL002",
      "protocol": "ocpp1.6",
      "evses": [
        { "evse_index": 1, "connector_count": 2 }
      ],
      "configuration": [
        {
          "name": "OCPPCommCtrlr",
          "variables": [
            { "name": "MeterValueSampleInterval", "value": "20", "required": true }
          ]
        }
      ]
    }
  ]')

CP1=$(echo "$RESPONSE" | jq -r '.cp_ids[0]')
CP2=$(echo "$RESPONSE" | jq -r '.cp_ids[1]')

echo "Created CP1 with ID: $CP1"
echo "Created CP2 with ID: $CP2"

# 2. Schedule events for both charge points simultaneously
curl -X POST http://localhost:3000/cp/events \
  -H "Content-Type: application/json" \
  -d '{
    "'"$CP1"'": [
      {
        "delay": 0,
        "event_type": "communication_start"
      },
      {
        "delay": 2,
        "event_type": "authorize",
        "id_tag": "VEHICLE001"
      },
      {
        "delay": 3,
        "event_type": "plug",
        "evse_index": 1,
        "connector_id": 1
      },
      {
        "delay": 5,
        "event_type": "start_transaction",
        "id_tag": "VEHICLE001",
        "evse_index": 1,
        "connector_id": 1,
        "meter_start": 10000
      },
      {
        "delay": 15,
        "event_type": "stop_transaction",
        "evse_index": 1,
        "connector_id": 1,
        "meter_stop": 15000
      },
      {
        "delay": 20,
        "event_type": "communication_stop"
      }
    ],
    "'"$CP2"'": [
      {
        "delay": 1,
        "event_type": "communication_start"
      },
      {
        "delay": 4,
        "event_type": "authorize",
        "id_tag": "VEHICLE002"
      },
      {
        "delay": 6,
        "event_type": "plug",
        "evse_index": 1,
        "connector_id": 1
      },
      {
        "delay": 8,
        "event_type": "start_transaction",
        "id_tag": "VEHICLE002",
        "evse_index": 1,
        "connector_id": 1,
        "meter_start": 20000
      },
      {
        "delay": 25,
        "event_type": "stop_transaction",
        "evse_index": 1,
        "connector_id": 1,
        "meter_stop": 28000
      },
      {
        "delay": 30,
        "event_type": "communication_stop"
      }
    ]
  }'

# 3. Retrieve CP1 data
echo "=== CP1 Status ==="
curl http://localhost:3000/cp/$CP1/data | jq

# 4. Retrieve CP2 data
echo "=== CP2 Status ==="
curl http://localhost:3000/cp/$CP2/data | jq

# 5. List all CPs
echo "=== All Charge Points ==="
curl http://localhost:3000/cp | jq
```

**Key Features Demonstrated**:
- **Bulk Creation**: Create multiple charge points in a single API call using array syntax
- **Simultaneous Multi-CP Events**: Schedule staggered events for both chargers using a dictionary structure
- **Delay Semantics**: `delay: 0` triggers immediately; `delay: 5` triggers 5 seconds after start
- **Zero Authentication**: Creates CPs without username/password (uses defaults for non-profile-6 deployments)
- **Sequential Event Processing**: Both chargers execute their event sequences independently and concurrently

## Data Structures

### CPDataResponse

Represents the current state of a Charge Point:

```rust
pub struct CPDataResponse {
    pub cp_id: usize,
    pub username: Option<String>,
    pub password: Option<String>,
    pub vendor: String,
    pub model: String,
    pub serial: String,
    pub protocol: String,
    pub booted: bool,
    pub events: Vec<ScheduledEvent>,
}
```

### ScheduledEvent

Represents a scheduled event:

```rust
pub struct ScheduledEvent {
    pub delay: u32,                 // Delay in seconds from start (0 = trigger now)
    pub event_type: String,         // Type of event
    pub id_tag: Option<String>,     // RFID tag for authorization
    pub evse_index: Option<u32>,    // EVSE index for plug/unplug
    pub connector_id: Option<u32>,  // Connector ID for plug/unplug
    pub meter_start: Option<u32>,   // Meter value at start of transaction
    pub meter_stop: Option<u32>,    // Meter value at stop of transaction
}
```

## Error Handling

All error responses follow this format:

```json
{
  "error": "Error message",
  "details": "Detailed explanation (optional)"
}
```

Common HTTP Status Codes:
- `201 Created`: Resource created successfully
- `200 OK`: Request succeeded
- `400 Bad Request`: Invalid request format
- `404 Not Found`: Resource not found
- `500 Internal Server Error`: Server error

## Thread Safety

The API is fully thread-safe:
- `Arc<CPStore>` for shared ownership across threads
- `RwLock` for safe concurrent reads and writes
- `AtomicUsize` for lock-free ID generation

## Implementation Notes

The REST API is designed as a reference and demonstration of:
1. Creating and managing Charge Point instances
2. Storing configuration data (credentials, vendor info, etc.)
3. Managing scheduled events that can be used to simulate charge point behavior
4. Thread-safe concurrent access patterns in Rust

For production use, consider:
- Adding persistent storage (database)
- Implementing authentication/authorization
- Adding request validation and error handling
- Implementing logging and monitoring
- Adding rate limiting
- Using HTTPS/TLS
