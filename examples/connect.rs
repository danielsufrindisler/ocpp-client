use ocpp_client::communicator1_6::OCPPCommunicator1_6;
use ocpp_client::communicator2_0_1::OCPPCommunicator2_0_1;
use ocpp_client::communicator_trait::OCPPCommunicator;
use ocpp_client::cp_data::{
    AuthorizationType, CPData, ChargeSession, ChargeSessionReference, EV, EVSE, MessageReference, PlugAndCharge, RFID, TransitionGraph, UserStateTransitions, UserTransition::{self, *}
};
use ocpp_client::ocpp_deque::OCPPDeque;
use ocpp_client::Client::{OCPP1_6, OCPP2_0_1};
use ocpp_client::{connect, ConnectOptions};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

struct CP {
    communicator: Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>>,
    data: Arc<Mutex<CPData>>,
    client: Option<ocpp_client::Client>,
}

impl CP {
    pub fn communicator(&self) -> Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>> {
        self.communicator.clone()
    }

    pub async fn run(
        &mut self,
        address: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data = self.data.lock().await;
        let options: ConnectOptions = ConnectOptions {
            username: data.username.as_deref(),
            password: data.password.as_deref(),
        };

        let client = connect(address, Some(options)).await?;
        drop(data);

        // Create mpsc channel
        let (trigger_message_requests, mut rx) = mpsc::channel::<String>(100);

        match &client {
            OCPP1_6(inner) => {
                *self.communicator.lock().await = Some(Box::new(OCPPCommunicator1_6 {
                    client: inner.clone(),
                    data: self.data.clone(),
                    trigger_message_requests: Some(trigger_message_requests),
                    ocpp_deque: OCPPDeque::new(inner.clone().base.clone()),
                })
                    as Box<dyn OCPPCommunicator>);
                println!("Connected using OCPP 1.6");
            }
            OCPP2_0_1(inner) => {
                *self.communicator.lock().await = Some(Box::new(OCPPCommunicator2_0_1 {
                    client: inner.clone(),
                    data: self.data.clone(),
                    trigger_message_requests: Some(trigger_message_requests),
                    ocpp_deque: OCPPDeque::new(inner.clone().base.clone()),
                })
                    as Box<dyn OCPPCommunicator>);
                println!("Connected using OCPP 2.0.1");
            }
        };

        self.client = Some(client);

        // Spawn task to listen for messages on the mpsc channel
        let communicator = self.communicator.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if msg == "boot_notification" {
                    println!("Received boot_notification message from mpsc");
                    if let Some(comm) = communicator.lock().await.as_ref() {
                        let _ = comm.send_boot_notification().await;
                    }
                }
            }
        });

        if let Some(comm) = self.communicator.lock().await.as_ref() {
            comm.register_messages().await;
            comm.send_boot_notification().await?
        }

        let evse_count = self.data.lock().await.evses.len();
        for i in 0..evse_count {
            let cp_data_clone = self.data.clone();

            let comm_arc = self.communicator.clone();
            println!(
                "spawning evse task for EVSE with {} connector(s)",
                cp_data_clone.lock().await.evses[i].connector_ids.len()
            );

            tokio::spawn(async move {
                let mut t = 0;
                println!(
                    "spawned evse task for EVSE with {} connector(s)",
                    cp_data_clone.lock().await.evses[i].connector_ids.len()
                );
                let comm_arc2 = comm_arc.clone();
                let session_count = cp_data_clone.lock().await.evses[i].charge_sessions.len();
                for j in 0..session_count {
                    let transitions = &cp_data_clone.lock().await.evses[i].charge_sessions[j]
                        .user_transitions
                        .transitions
                        .clone();
                    for transition in transitions {
                        println!("2");

                        tokio::time::sleep(tokio::time::Duration::from_secs(
                            transition.duration as u64,
                        ))
                        .await;

                        t += transition.duration;
                        println!("Transitioning at time {} {:?}", t, transition.transition);

                        if transition.transition == UserTransition::LocalStart {
                            if let Some(comm) = comm_arc2.lock().await.as_ref() {
                                let reference: MessageReference =
                                    MessageReference::ChargeSession(ChargeSessionReference {
                                        evse_index: i,
                                        charge_session_index: j,
                                    });
                                let _ = comm
                                    .send_authorize(
                                        cp_data_clone.lock().await.evses[i].charge_sessions[j]
                                            .authorization
                                            .clone(),
                                        reference,
                                    )
                                    .await;
                            }
                        }
                    }
                }
            });
        }

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
            password: Some("RustyRust".to_string()),
            serial: "RustTest002".to_string(),
            model: "RustModel".to_string(),
            vendor: "RustVendor".to_string(),
            booted: false,
            evses: vec![evse],
        })),
    };

    cp1.run("wss://ocpp.coreevi.com/ocpp2.0.1/65/RustTest002")
        .await?;

    Ok(())
}
