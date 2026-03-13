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

## API Endpoints

### 1. Create a Charge Point

**Endpoint**: `POST /cp`

Creates a new Charge Point with the provided configuration.

**Request Body**:
```json
{
  "username": "TestCP001",
  "password": "secure_password",
  "vendor": "TestVendor",
  "model": "TestModel",
  "serial": "SN12345",
  "protocol": "ocpp1.6"
}
```

**Response** (201 Created):
```json
{
  "cp_id": 0,
  "message": "CP 0 created successfully"
}
```

**Example**:
```bash
curl -X POST http://localhost:3000/cp \
  -H "Content-Type: application/json" \
  -d '{
    "username": "TestCP001",
    "password": "secure_password",
    "vendor": "TestVendor",
    "model": "TestModel",
    "serial": "SN12345",
    "protocol": "ocpp1.6"
  }'
```

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
    "events": []
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

**Endpoint**: `GET /cp/{cp_id}/data`

Retrieves the current data for a specific Charge Point, including configuration and scheduled events.

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
  "events": [
    {
      "duration": 5,
      "event_type": "authorize",
      "id_tag": "TAG123"
    }
  ]
}
```

**Error Response** (404 Not Found):
```json
{
  "error": "CP not found",
  "details": "No CP with ID 999"
}
```

**Example**:
```bash
curl http://localhost:3000/cp/0/data
```

---

### 4. Add a Scheduled Event to a Charge Point

**Endpoint**: `POST /cp/{cp_id}/events`

Adds a scheduled event to a Charge Point. The event will trigger at the specified duration from start.

**Request Body**:
```json
{
  "duration": 5,
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

| Event Type | Required Fields | Optional Fields |
|---|---|---|
| `authorize` | `id_tag` | - |
| `plug` | `evse_index`, `connector_id` | - |
| `unplug` | `evse_index`, `connector_id` | - |
| `communication_start` | - | - |
| `communication_stop` | - | - |
| `local_stop` | - | - |

**Examples**:

Authorization event:
```bash
curl -X POST http://localhost:3000/cp/0/events \
  -H "Content-Type: application/json" \
  -d '{
    "duration": 5,
    "event_type": "authorize",
    "id_tag": "TAG123"
  }'
```

Plug event:
```bash
curl -X POST http://localhost:3000/cp/0/events \
  -H "Content-Type: application/json" \
  -d '{
    "duration": 10,
    "event_type": "plug",
    "evse_index": 1,
    "connector_id": 1
  }'
```

Communication event:
```bash
curl -X POST http://localhost:3000/cp/0/events \
  -H "Content-Type: application/json" \
  -d '{
    "duration": 2,
    "event_type": "communication_stop"
  }'
```

---

## Complete Workflow Example

```bash
# 1. Create a CP
CP_ID=$(curl -s -X POST http://localhost:3000/cp \
  -H "Content-Type: application/json" \
  -d '{
    "username": "RustTest001",
    "password": "SecurePass123",
    "vendor": "RustVendor",
    "model": "RustModel",
    "serial": "RN123456",
    "protocol": "ocpp1.6"
  }' | jq -r '.cp_id')

echo "Created CP with ID: $CP_ID"

# 2. Add authorization event (5 seconds)
curl -X POST http://localhost:3000/cp/$CP_ID/events \
  -H "Content-Type: application/json" \
  -d '{
    "duration": 5,
    "event_type": "authorize",
    "id_tag": "TAG123"
  }'

# 3. Add plug event (10 seconds)
curl -X POST http://localhost:3000/cp/$CP_ID/events \
  -H "Content-Type: application/json" \
  -d '{
    "duration": 10,
    "event_type": "plug",
    "evse_index": 1,
    "connector_id": 1
  }'

# 4. Add communication stop event (15 seconds)
curl -X POST http://localhost:3000/cp/$CP_ID/events \
  -H "Content-Type: application/json" \
  -d '{
    "duration": 15,
    "event_type": "communication_stop"
  }'

# 5. Add communication start event (25 seconds)
curl -X POST http://localhost:3000/cp/$CP_ID/events \
  -H "Content-Type: application/json" \
  -d '{
    "duration": 25,
    "event_type": "communication_start"
  }'

# 6. Retrieve CP data
curl http://localhost:3000/cp/$CP_ID/data | jq

# 7. List all CPs
curl http://localhost:3000/cp | jq
```

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
    pub duration: u32,              // Duration in seconds from start
    pub event_type: String,         // Type of event
    pub id_tag: Option<String>,     // RFID tag for authorization
    pub evse_index: Option<u32>,    // EVSE index for plug/unplug
    pub connector_id: Option<u32>,  // Connector ID for plug/unplug
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
