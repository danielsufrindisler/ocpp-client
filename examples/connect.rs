use log::{debug, error, info, log_enabled, Level};
use ocpp_client::communicator1_6::OCPPCommunicator1_6;
use ocpp_client::communicator2_0_1::OCPPCommunicator2_0_1;
use ocpp_client::communicator_trait::OCPPCommunicator;
use ocpp_client::cp_data::{
    AuthorizationType, CPData, ChargeSession, ChargeSessionReference,
    EventTypes::{self, *},
    MessageReference, ScheduledEvents, EV, EVSE, RFID,
};
use ocpp_client::ocpp_1_6::OCPP1_6Client;
use ocpp_client::ocpp_2_0_1::OCPP2_0_1Client;
use ocpp_client::ocpp_deque::OCPPDeque;
use ocpp_client::Client::{OCPP1_6, OCPP2_0_1};
use ocpp_client::{setup_socket, ConnectOptions};
use rustls::client;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};

struct CP {
    communicator: Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>>,
    data: Arc<Mutex<CPData>>,
    client: Option<ocpp_client::Client>,
}

impl CP {
    pub async fn run(
        &mut self,
        address: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get connection info from data
        let data = self.data.lock().await;

        let selected_protocol = if let Some(ref proto) = data.selected_protocol {
            proto.clone()
        } else {
            let supported = data
                .supported_protocol_versions
                .clone()
                .unwrap_or("ocpp1.6".to_string());
            supported
                .split(',')
                .next()
                .unwrap_or("ocpp1.6")
                .trim()
                .to_string()
        };

        let options_username = data.username.clone();
        let options_password = data.password.clone();
        let data_clone = self.data.clone();
        let (trigger_message_requests, rx) = mpsc::channel::<String>(100);

        drop(data);

        // Create client and communicator immediately (unconnected)
        match selected_protocol.as_str() {
            "ocpp1.6" => {
                let client = OCPP1_6Client::new_unconnected(
                    options_username,
                    options_password,
                    address.to_string(),
                    selected_protocol.to_string(),
                );
                let base_clone = client.base.clone();
                *self.communicator.lock().await = Some(Box::new(OCPPCommunicator1_6 {
                    client: client.clone(),
                    data: data_clone,
                    trigger_message_requests: Some(trigger_message_requests),
                    ocpp_deque: OCPPDeque::new(client.base.clone()),
                })
                    as Box<dyn OCPPCommunicator>);
                self.client = Some(OCPP1_6(client));
            }
            "ocpp2.0.1" => {
                let client = OCPP2_0_1Client::new_unconnected(
                    options_username,
                    options_password,
                    address.to_string(),
                    selected_protocol.to_string(),
                );
                let base_clone = client.base.clone();
                *self.communicator.lock().await = Some(Box::new(OCPPCommunicator2_0_1 {
                    client: client.clone(),
                    data: data_clone,
                    trigger_message_requests: Some(trigger_message_requests),
                    ocpp_deque: OCPPDeque::new(client.base.clone()),
                })
                    as Box<dyn OCPPCommunicator>);
                self.client = Some(OCPP2_0_1(client));
            }
            _ => {
                return Err("Unsupported protocol".into());
            }
        }



        let comm_mpsc = self.communicator.clone();

        comm_mpsc.lock().await.as_ref().unwrap().register_messages().await;

        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(msg) = rx.recv().await {
                if msg == "boot_notification" {
                    debug!("Received boot_notification trigger from mpsc");
                    if let Some(comm) = comm_mpsc.lock().await.as_ref() {
                        let _ = comm.send_boot_notification().await;
                    }
                }
            }
        });

        //todo, this needs to not happen yet... connectio not established
        if let Some(comm) = self.communicator.lock().await.as_ref() {
            comm.send_boot_notification().await.ok();
        }

        // Spawn EVSE tasks immediately
        self.spawn_evse_tasks().await;
        // Wait a bit for connection to establish
        tokio::time::sleep(Duration::from_secs(2)).await;

        info!("Press Ctrl-C to exit");
        tokio::signal::ctrl_c().await?;

        info!("Disconnecting...");
        if let Some(ocpp_client::Client::OCPP1_6(client)) = &self.client {
            let _ = client.disconnect().await;
        } else if let Some(ocpp_client::Client::OCPP2_0_1(client)) = &self.client {
            let _ = client.disconnect().await;
        }

        Ok(())
    }

    async fn spawn_evse_tasks(&self) {
        let evse_count = self.data.lock().await.evses.len();
        let cp_data_clone = self.data.clone();
        let comm_arc = self.communicator.clone();

        let cp_data = cp_data_clone.clone();
        let comm = comm_arc.clone();

        tokio::spawn(async move {
            let mut t = 0;
            let comm_arc2 = comm.clone();
            let events = cp_data.lock().await.events.clone();
            for (i, event) in events.iter().enumerate() {
                tokio::time::sleep(tokio::time::Duration::from_secs(event.duration as u64)).await;

                t += event.duration;
                info!("Event ocurring at time {} {:?}", t, event.event);
                match &event.event {
                    EventTypes::Authorize(authorization) => {
                        // Try to lock with retry logic
                        loop {
                            match comm_arc2.try_lock() {
                                Ok(guard) => {
                                    if let Some(comm) = guard.as_ref() {
                                        let _ = comm
                                            .send_authorize(
                                                authorization.clone(),
                                                MessageReference::EventIndex(i),
                                            )
                                            .await;
                                    }
                                    break;
                                }
                                Err(_) => {
                                    // Lock not available, sleep and retry
                                    tokio::time::sleep(Duration::from_millis(10)).await;
                                }
                            }
                        }
                    }
                    EventTypes::LocalStop => {
                        // todo, if charging, stop charging and transition to finishing
                    }
                    EventTypes::Plug(reference) => {
                        cp_data.lock().await.evses[reference.evse_index].charge_sessions
                            [reference.charge_session_index]
                            .plugged_in = true;
                        CP::check_start_charging(
                            cp_data.clone(),
                            reference.evse_index,
                            reference.charge_session_index,
                            comm.clone(),
                        )
                        .await
                    }
                    EventTypes::Unplug(reference) => {
                        cp_data.lock().await.evses[reference.evse_index].charge_sessions
                            [reference.charge_session_index]
                            .plugged_in = false;
                    }
                    EventTypes::RemoteStart(reference) => {
                        // todo: handle remote start
                    }
                    EventTypes::RemoteStop(reference) => {
                        // todo: handle remote stop
                    }
                    EventTypes::CommunicationStart => {
                        info!("Simulated communication restart");
                        loop {
                            match comm_arc2.try_lock() {
                                Ok(mut guard) => {
                                    if let Some(comm) = guard.as_mut() {
                                        comm.get_base().establish_connection();
                                        info!("Simulated communication restart");
                                    }
                                    break;
                                }
                                Err(_) => {
                                    // Lock not available, sleep and retry
                                    tokio::time::sleep(Duration::from_millis(10)).await;
                                }
                            }
                            // Simulate communication loss by dropping communicator reference
                        }
                    }

                    EventTypes::CommunicationStop => {
                        loop {
                            match comm_arc2.try_lock() {
                                Ok(mut guard) => {
                                    if let Some(comm) = guard.as_mut() {
                                        let handle = comm.get_base().handle.clone();
                                        if let Some(task_handle) = handle.lock().await.take() {
                                            task_handle.abort();
                                            comm.get_base().sink.lock().await.take();
                                            comm.get_base().stream.lock().await.take();

                                            info!("Simulated communication loss: connection ");
                                        };
                                    }
                                    break;
                                }
                                Err(_) => {
                                    // Lock not available, sleep and retry
                                    tokio::time::sleep(Duration::from_millis(10)).await;
                                }
                            }
                            // Simulate communication loss by dropping communicator reference
                        }
                    }
                }
            }
        });
    }

    #[allow(dead_code)]
    async fn check_stop_charging(
        cp_data: Arc<Mutex<CPData>>,
        evse_index: usize,
        charge_session_index: usize,
        communicator: Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>>,
    ) {
        let mut data = cp_data.lock().await;
        let cs = &mut data.evses[evse_index].charge_sessions[charge_session_index];
        if cs.started {
            cs.started = false;
            drop(data);
            if let Some(_comm) = communicator.lock().await.as_ref() {
                let _reference: MessageReference =
                    MessageReference::ChargeSession(ChargeSessionReference {
                        evse_index,
                        charge_session_index,
                        connector_id: 1, //todo
                    });
                //let _ = _comm.send_stop_transaction(_reference).await;
            }
        }
    }

    async fn check_start_charging(
        cp_data: Arc<Mutex<CPData>>,
        evse_index: usize,
        charge_session_index: usize,
        communicator: Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>>,
    ) {
        debug!(
            "Checking if can start charging for evse {}, session {}",
            evse_index, charge_session_index
        );
        let mut data = cp_data.lock().await;
        let cs = &data.evses[evse_index].charge_sessions[charge_session_index];
        debug!("Checking if can start charging for evse {}, session {}. Authorized: {}, Plugged in: {}, Started: {}",
            evse_index, charge_session_index, data.authorization.is_some(), cs.plugged_in, cs.started);
        if data.authorization.is_some() && cs.plugged_in && !cs.started {
            data.evses[evse_index].charge_sessions[charge_session_index].started = true;
            drop(data);

            if let Some(comm) = communicator.lock().await.as_ref() {
                let reference: ChargeSessionReference = ChargeSessionReference {
                    evse_index,
                    charge_session_index,
                    connector_id: 1, //todo
                };
                info!(
                    "Starting charging for evse {}, session {}",
                    evse_index, charge_session_index
                );
                let _ = comm.send_start_transaction(reference).await;
                let mut data = cp_data.lock().await;
                data.authorization = None; // Clear authorization after starting charging, for testing purposes
            }
        } else {
            debug!(
                "Cannot start charging for evse {}, session {}. Authorized: {}, Plugged in: {}, Started: {}",
                evse_index, charge_session_index, data.authorization.is_some(), cs.plugged_in, cs.started
            );
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "INFO");
    }

    env_logger::init();

    let auth1 = AuthorizationType::RFID(RFID {
        id_tag: "TAG123".to_string(),
    });

    let ev: EV = EV {
        power_vs_soc: vec![(0.0, 50.0), (80.0, 30.0), (100.0, 0.0)],
    };

    let cs1 = ChargeSession {
        ev: ev,
        authorization: None,
        plugged_in: false,
        started: false,
    };

    let evse: EVSE = EVSE {
        is_ac: false,
        connector_ids: vec![1, 2],
        charge_sessions: vec![cs1],
    };

    let mut cp1 = CP {
        communicator: Arc::new(Mutex::new(None)),
        client: None,

        data: Arc::new(Mutex::new(CPData {
            username: Some("RustTest002".to_string()),
            password: Some("RustyRustRustRust".to_string()),
            supported_protocol_versions: Some("ocpp1.6, ocpp2.0.1".to_string()),
            selected_protocol: Some("ocpp1.6".to_string()), // Pre-select protocol before connection
            serial: "RustTest002".to_string(),
            model: "RustModel".to_string(),
            vendor: "RustVendor".to_string(),
            booted: false,
            evses: vec![evse],
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
                        evse_index: 0,
                        connector_id: 2,
                        charge_session_index: 0,
                    }),
                },
                ScheduledEvents {
                    duration: 100,
                    event: EventTypes::Unplug(ChargeSessionReference {
                        evse_index: 0,
                        connector_id: 2,
                        charge_session_index: 0,
                    }),
                },
            ],
            authorization: None,
        })),
    };
    //"wss://ocpp.coreevi.com/ocpp2.0.1/65/RustTest002"

    cp1.run("ws://127.0.0.1:8180/steve/websocket/CentralSystemService/RustTest002")
        .await?;

    Ok(())
}
