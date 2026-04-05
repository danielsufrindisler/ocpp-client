# Acknowledgement
This was forked from https://github.com/flowionab/ocpp-client A huge portion of this repository is still in the original format from the flowionab version and wouldn't be where it is today without a great base project.  

## Overview

`ocpp-client` is a Rust library that provides an OCPP (Open Charge Point Protocol) client implementation. This library enables developers to integrate with central system (CSMS) that use the OCPP protocol, allowing for seamless communication and efficient.

## Planned Features

- **Fully functioning charger simulators which can simulate users conecting, charging their vehicles, paying by RFID, and more.
- **OCPP 1.6, 2.0.1, 2.1 support**: Communicate with OCPP 1.6 or 2.0.1 JSON compliant servers (CSMS).
- **Async/Await support**: Built using asynchronous Rust for high performance and scalability.
- **Customizable**: Easily extendable to add custom message handlers or to support additional OCPP features.
- **Error Handling**: Robust error handling and logging to assist in debugging and maintenance.
- **Comprehensive Documentation**: Detailed documentation with examples to get you started quickly.
- API access for automation:  Setup chargers, and send events (plugin, RFID etc) through API calls



## Installation

Add the following to your `Cargo.toml`:

```toml
[dependencies]
ocpp-client = "0.1"
```

## Test Automation

The project includes automated testing capabilities:

### REST API Server
Start the REST API server for programmatic control:
```bash
cargo run --example connect -- --server
```

The server provides:
- Interactive API documentation at `http://127.0.0.1:3000/swagger-ui`
- Endpoints for creating charge points and triggering events
- Message statistics tracking at `/summary`

### Test Configuration

The project uses `data/test_all_cps.json` which defines 4 different CP types:

**CP_1_Full**: Complete implementation with all features (events, variables, components)
**CP_2_NoEvents**: Minimal CP for testing core functionality without events
**CP_3_ComponentsOnly**: CP focused on component-based device model
**CP_4_RESTOnly**: CP created via REST API at runtime (not in test config file)

### Automated Test Script
Run automated tests with all 4 CP types:
```bash
python3 test_automation.py
```

This script:
1. Starts the OCPP test server with `data/test_all_cps.json`
2. Waits for initialization
3. Creates CP_4 via the `/cp/from-rest` API endpoint
4. Triggers plug-in and authorize events for all CPs
5. Retrieves and parses complete message statistics
6. Displays results for each CP type

See `test_automation.py` for implementation details.

## Organization / Architecture

/srs/ocpp_1_6, src/ocpp_2_0_1
functions to send messages to a CPMS, or to handle requests from a CPMS

/src/common_client
common functions to send raw messages to a CPMS and receive raw messages frp, a CPMS

/src/reconnectws
stream-reconnect websocket details so that a client can connect to a server, and keep reconnecting as needed

/src/ocpp_deque.rs 
This *will* be a custom ocpp queue with the ability to have different retry strategies, different persistence, and different priorities for different message types
for ocpp1.6 it will also handle changing transaction ids from an internal placeholder to actual values

/examples/connect.rs
This is the main file for now.  It will be split up into several different modules in the future.  It has the business logic for a charger, and ocpp1.6 and ocpp2.0.1 specific message senders and handlers.  The goal is to have the same business logic, but slightly different communication between the different ocpp versions.  


## License

This project is licensed under the MIT License. See the [LICENSE](https://github.com/danielsufrin-disler/ocpp-client/blob/main/LICENSE) file for details.


## TODOs
fauult events
file is only for config keys
file is for nothing




