# Acknowledgement
This was forked from https://github.com/flowionab/ocpp-client A huge portion of this repository is still in the original format from the flowionab version and wouldn't be where it is today without a great base project.  

## Overview

`ocpp-client` is a Rust library that provides an OCPP (Open Charge Point Protocol) client implementation. This library enables developers to integrate with central system (CSMS) that use the OCPP protocol, allowing for seamless communication and efficient.

## Features

- **OCPP 1.6 and 2.0.1 support**: Communicate with OCPP 1.6 or 2.0.1 JSON compliant servers (CSMS).
- **Async/Await support**: Built using asynchronous Rust for high performance and scalability.
- **Customizable**: Easily extendable to add custom message handlers or to support additional OCPP features.
- **Error Handling**: Robust error handling and logging to assist in debugging and maintenance.
- **Comprehensive Documentation**: Detailed documentation with examples to get you started quickly.

## Installation

Add the following to your `Cargo.toml`:

```toml
[dependencies]
ocpp-client = "0.1"
```

## Usage

see examples/connect.rs

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

