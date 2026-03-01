use ocpp_client::communicator1_6::OCPPCommunicator1_6;
use ocpp_client::communicator2_0_1::OCPPCommunicator2_0_1;
use ocpp_client::communicator_trait::OCPPCommunicator;
use ocpp_client::cp_data::{
    AuthorizationType, CPData, ChargeSession, ChargeSessionReference, EV, EVSE, MessageReference, RFID, TransitionGraph, UserStateTransitions, UserTransition::{self, *}
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
            let supported = data.supported_protocol_versions.clone().unwrap_or("ocpp1.6".to_string());
            supported.split(',').next().unwrap_or("ocpp1.6").trim().to_string()
        };
        
        let options_username = data.username.clone();
        let options_password = data.password.clone();
        let data_clone = self.data.clone();
        let (trigger_message_requests, rx) = mpsc::channel::<String>(100);

        drop(data);

        // Create client and communicator immediately (unconnected)
        match selected_protocol.as_str() {
            "ocpp1.6" => {
                self.setup_ocpp1_6(
                    data_clone,
                    address,
                    &selected_protocol,
                    options_username.clone(),
                    options_password.clone(),
                    trigger_message_requests,
                ).await?;
            }
            "ocpp2.0.1" => {
                self.setup_ocpp2_0_1(
                    data_clone,
                    address,
                    &selected_protocol,
                    options_username.clone(),
                    options_password.clone(),
                    trigger_message_requests,
                ).await?;
            }
            _ => {
                return Err("Unsupported protocol".into());
            }
        }

        let comm_mpsc = self.communicator.clone();
        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(msg) = rx.recv().await {
                if msg == "boot_notification" {
                    println!("Received boot_notification trigger from mpsc");
                    if let Some(comm) = comm_mpsc.lock().await.as_ref() {
                        let _ = comm.send_boot_notification().await;
                    }
                }
            }
        });
        if let Some(comm) = self.communicator.lock().await.as_ref() {
            comm.send_boot_notification().await.ok();
        }
        if let Some(ocpp_client::Client::OCPP1_6(client)) = &self.client {
            client.base.spawn_connection_task(address, &selected_protocol, options_username.clone(), options_password.clone());
        } else if let Some(ocpp_client::Client::OCPP2_0_1(client)) = &self.client {
            client.base.spawn_connection_task(address, &selected_protocol, options_username.clone(), options_password.clone());
        }
        // Spawn EVSE tasks immediately
        self.spawn_evse_tasks().await;
        // Wait a bit for connection to establish
        tokio::time::sleep(Duration::from_secs(2)).await;
        
        println!("Connected! Waiting for messages...");

        println!("Press Ctrl-C to exit");
        tokio::signal::ctrl_c().await?;

        println!("Disconnecting...");
        if let Some(ocpp_client::Client::OCPP1_6(client)) = &self.client {
            let _ = client.disconnect().await;
        } else if let Some(ocpp_client::Client::OCPP2_0_1(client)) = &self.client {
            let _ = client.disconnect().await;
        }

        Ok(())
    }



    async fn setup_ocpp1_6(
        &mut self,
        data: Arc<Mutex<CPData>>,
        address: &str,
        protocol: &str,
        options_username: Option<String>,
        options_password: Option<String>,
        trigger_message_requests: mpsc::Sender<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = OCPP1_6Client::new_unconnected();
        let base_clone = client.base.clone();

        *self.communicator.lock().await = Some(Box::new(OCPPCommunicator1_6 {
            client: client.clone(),
            data,
            trigger_message_requests: Some(trigger_message_requests),
            ocpp_deque: OCPPDeque::new(client.base.clone()),
        }) as Box<dyn OCPPCommunicator>);


        self.client = Some(OCPP1_6(client));
        Ok(())
    }

    async fn setup_ocpp2_0_1(
        &mut self,
        data: Arc<Mutex<CPData>>,
        address: &str,
        protocol: &str,
        options_username: Option<String>,
        options_password: Option<String>,
        trigger_message_requests: mpsc::Sender<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = OCPP2_0_1Client::new_unconnected();
        let base_clone = client.base.clone();
        
        *self.communicator.lock().await = Some(Box::new(OCPPCommunicator2_0_1 {
            client: client.clone(),
            data,
            trigger_message_requests: Some(trigger_message_requests),
            ocpp_deque: OCPPDeque::new(client.base.clone()),
        }) as Box<dyn OCPPCommunicator>);


        self.client = Some(OCPP2_0_1(client));
        Ok(())
    }

    async fn spawn_evse_tasks(&self) {
        let evse_count = self.data.lock().await.evses.len();
        let cp_data_clone = self.data.clone();
        let comm_arc = self.communicator.clone();
        
        for i in 0..evse_count {
            let cp_data = cp_data_clone.clone();
            let comm = comm_arc.clone();

            println!(
                "spawning evse task for EVSE with {} connector(s)",
                cp_data.lock().await.evses[i].connector_ids.len()
            );

            tokio::spawn(async move {
                let mut t = 0;
                println!(
                    "spawned evse task for EVSE with {} connector(s)",
                    cp_data.lock().await.evses[i].connector_ids.len()
                );
                
                let comm_arc2 = comm.clone();
                let session_count = cp_data.lock().await.evses[i].charge_sessions.len();
                
                for j in 0..session_count {
                    let transitions = &cp_data.lock().await.evses[i].charge_sessions[j]
                        .user_transitions
                        .transitions
                        .clone();
                    for transition in transitions {
                        tokio::time::sleep(tokio::time::Duration::from_secs(
                            transition.duration as u64,
                        ))
                        .await;

                        t += transition.duration;
                        println!("Transitioning at time {} {:?}", t, transition.transition);

                        match transition.transition {
                            UserTransition::LocalStart => {
                                // Try to lock with retry logic
                                loop {
                                    match comm_arc2.try_lock() {
                                        Ok(guard) => {
                                            if let Some(comm) = guard.as_ref() {
                                                let reference: MessageReference =
                                                    MessageReference::ChargeSession(ChargeSessionReference {
                                                        evse_index: i,
                                                        charge_session_index: j,
                                                    });
                                                let _ = comm
                                                    .send_authorize(
                                                        cp_data.lock().await.evses[i].charge_sessions[j]
                                                            .authorization
                                                            .clone(),
                                                        reference,
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
                            UserTransition::LocalStop => {
                                // todo, if charging, stop charging and transition to finishing
                            }
                            UserTransition::Plug => {
                                cp_data.lock().await.evses[i]
                                    .charge_sessions[j]
                                    .plugged_in = true;
                                CP::check_start_charging(
                                    cp_data.clone(),
                                    i,
                                    j,
                                    comm.clone(),
                                ).await
                            }
                            UserTransition::Unplug => {
                                cp_data.lock().await.evses[i]
                                    .charge_sessions[j]
                                    .plugged_in = false;
                            }
                            UserTransition::RemoteStart => {
                                // todo: handle remote start
                            }
                            UserTransition::RemoteStop => {
                                // todo: handle remote stop
                            }
                        }
                    }
                }
            });
        }
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
        let mut data = cp_data.lock().await;
        let cs = &mut data.evses[evse_index].charge_sessions[charge_session_index];
        if cs.authorized && cs.plugged_in && !cs.started {
            cs.started = true;
            drop(data);
            if let Some(comm) = communicator.lock().await.as_ref() {
                let reference: ChargeSessionReference =
                    ChargeSessionReference {
                        evse_index,
                        charge_session_index,
                    };
                println!(
                    "Starting charging for evse {}, session {}",
                    evse_index, charge_session_index
                );
                let _ = comm.send_start_transaction(reference).await;
            }
        } else {
            println!(
                "Cannot start charging for evse {}, session {}. Authorized: {}, Plugged in: {}, Started: {}",
                evse_index, charge_session_index, cs.authorized, cs.plugged_in, cs.started
            );
        }

    }

}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "trace");
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
        authorization: auth1,
        plugged_in: false,
        authorized: false,
        started: false,
        user_transitions: TransitionGraph {
            transitions: vec![
                UserStateTransitions {
                    duration: 5,
                    transition: LocalStart,
                },
                UserStateTransitions {
                    duration: 10,
                    transition: Plug,
                },
                UserStateTransitions {
                    duration: 100,
                    transition: Unplug,
                },
            ],
        },
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
            supported_protocol_versions: Some ("ocpp1.6, ocpp2.0.1".to_string()),
            selected_protocol: Some("ocpp1.6".to_string()), // Pre-select protocol before connection
            serial: "RustTest002".to_string(),
            model: "RustModel".to_string(),
            vendor: "RustVendor".to_string(),
            booted: false,
            evses: vec![evse],
        })),
    };
    //"wss://ocpp.coreevi.com/ocpp2.0.1/65/RustTest002"
    
    cp1.run("ws://127.0.0.1:8180/steve/websocket/CentralSystemService/RustTest002")
        .await?;

    Ok(())
}
