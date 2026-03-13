use log::{debug, info};
use crate::communicator1_6::OCPPCommunicator1_6;
use crate::communicator2_0_1::OCPPCommunicator2_0_1;
use crate::communicator_trait::OCPPCommunicator;
use crate::cp_data::{
    AuthorizationType, CPData, ChargeSession, ChargeSessionReference,
    EventTypes, GetVariableData, MessageReference, ScheduledEvents, EV, EVSE, RFID,
};
use crate::ocpp_1_6::OCPP1_6Client;
use crate::ocpp_2_0_1::OCPP2_0_1Client;
use crate::ocpp_deque::OCPPDeque;
use crate::Client::{OCPP1_6, OCPP2_0_1};
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};

pub struct CP {
    pub communicator: Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>>,
    pub data: Arc<Mutex<CPData>>,
    pub client: Option<crate::Client>,
}

impl CP {
    pub async fn run(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get connection info from data
        info!("running");

        let data = self.data.lock().await;

        let address = format!("{}{}", data.base_url, data.serial);

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
        info!("running");

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
                let _base_clone = client.base.clone();
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
                let _base_clone = client.base.clone();
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
        info!("running");

        let comm_mpsc = self.communicator.clone();

        comm_mpsc
            .lock()
            .await
            .as_ref()
            .unwrap()
            .register_messages()
            .await;

        info!("Communicator registered messages, starting background task to listen for triggers");
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
        info!("running 5");

        //todo, this needs to not happen yet... connectio not established
        if let Some(comm) = self.communicator.lock().await.as_ref() {
            comm.send_boot_notification().await.ok();
        }

        info!("spawning evse tasks immediately after boot notification");

        // Spawn EVSE tasks immediately
        self.spawn_evse_tasks().await;
        // Wait a bit for connection to establish
        /* 
        tokio::time::sleep(Duration::from_secs(2)).await;

        info!("Press Ctrl-C to exit");
        tokio::signal::ctrl_c().await?;

        info!("Disconnecting...");
        if let Some(crate::Client::OCPP1_6(client)) = &self.client {
            let _ = client.disconnect().await;
        } else if let Some(crate::Client::OCPP2_0_1(client)) = &self.client {
            let _ = client.disconnect().await;
        }
        */
        Ok(())
    }

    pub fn read_variables_from_file() -> GetVariableData {
        let path = "./data/device_model.json";
        let data = fs::read_to_string(path).expect("Unable to read file");
        let mut res: GetVariableData = serde_json::from_str(&data).expect("Unable to parse");
        //info!("{:?}", &res);

        for mut component in &mut res.components {
            let mut component_string = component.name.clone();
            if let Some(evse) = component.evse {
                component_string.push_str(evse.to_string().as_str());
            }
            if let Some(instance) = &component.instance {
                component_string.push_str(instance.as_str());
            }
            //                        + if component.connector.is_some() { component.connector.unwrap().to_string().as_str() } else { "" };

            for mut variable in &mut component.variables {
                if variable.ocpp16_key.is_none() {
                    variable.ocpp16_key = Some(component_string.clone() + variable.name.as_str());
                }
            }
        }

        res
    }

    pub fn list_components(data: GetVariableData) {
        for component in data.components {
            info!("Component: {}", component.name);
            if let Some(instance) = component.instance {
                info!("  Instance: {}", instance);
            }
            if let Some(evse) = component.evse {
                info!("  EVSE: {}", evse);
            }
            if let Some(connector) = component.connector {
                info!("  Connector: {}", connector);
            }
        }
    }

    async fn spawn_evse_tasks(&self) {
        debug!("Spawning EVSE task with data");

        let _evse_count = self.data.lock().await.evses.len();
        let cp_data_clone = self.data.clone();
        let comm_arc = self.communicator.clone();

        let cp_data = cp_data_clone.clone();
        let comm = comm_arc.clone();
        debug!("Spawning EVSE task with data");
        tokio::spawn(async move {
            let mut t = 0;
            let comm_arc2 = comm.clone();
            let mut finished_count = 0;
            loop {
            let events = cp_data.lock().await.events.clone();



            for (i, event) in events[finished_count..].iter().enumerate() {
                tokio::time::sleep(tokio::time::Duration::from_secs(event.duration as u64)).await;
                finished_count += 1;

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
                        cp_data
                            .lock()
                            .await
                            .evses
                            .get_mut(&reference.evse_index)
                            .unwrap()
                            .plugged_in = Some(reference.connector_id);
                        CP::check_start_charging(
                            cp_data.clone(),
                            reference.evse_index,
                            comm.clone(),
                        )
                        .await
                    }
                    EventTypes::Unplug(reference) => {
                        cp_data
                            .lock()
                            .await
                            .evses
                            .get_mut(&reference.evse_index)
                            .unwrap()
                            .plugged_in = None;
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
            tokio::time::sleep(Duration::from_millis(100)).await;

        }
        });
    }

    #[allow(dead_code)]
    async fn check_stop_charging(
        cp_data: Arc<Mutex<CPData>>,
        evse_index: u32,
        communicator: Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>>,
    ) {
        let mut data = cp_data.lock().await;
        if data.evses[&evse_index].started {
            data.evses.get_mut(&evse_index).unwrap().started = false;
            drop(data);
            if let Some(_comm) = communicator.lock().await.as_ref() {
                let _reference: MessageReference =
                    MessageReference::ChargeSession(ChargeSessionReference {
                        evse_index,
                        connector_id: 1, //todo
                    });
                //let _ = _comm.send_stop_transaction(_reference).await;
            }
        }
    }

    async fn check_start_charging(
        cp_data: Arc<Mutex<CPData>>,
        evse_index: u32,
        communicator: Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>>,
    ) {
        debug!("Checking if can start charging for evse {}", evse_index);
        let mut data = cp_data.lock().await;
        debug!("Checking if can start charging for evse {},  Authorized: {}, Plugged in: {:?}, Started: {:?}",
            evse_index, data.authorization.is_some(), data.evses[&evse_index].plugged_in, data.evses[&evse_index].started);
        if data.authorization.is_some()
            && data.evses[&evse_index].plugged_in.is_some()
            && !data.evses[&evse_index].started
        {
            data.evses.get_mut(&evse_index).unwrap().started = true;
            drop(data);

            if let Some(comm) = communicator.lock().await.as_ref() {
                let reference: ChargeSessionReference = ChargeSessionReference {
                    evse_index,
                    connector_id: 1, //todo
                };
                info!("Starting charging for evse {}", evse_index);
                let _ = comm.send_start_transaction(reference).await;
                let mut data = cp_data.lock().await;
                data.authorization = None; // Clear authorization after starting charging, for testing purposes
            }
        } else {
            debug!(
                "Cannot start charging for evse {}, Authorized: {}, Plugged in: {:?}, Started: {}",
                evse_index,
                data.authorization.is_some(),
                data.evses[&evse_index].plugged_in,
                data.evses[&evse_index].started
            );
        }
    }
}
