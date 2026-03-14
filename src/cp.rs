use crate::communicator1_6::OCPPCommunicator1_6;
use crate::communicator2_0_1::OCPPCommunicator2_0_1;
use crate::communicator_trait::OCPPCommunicator;
use crate::cp_data::{
    self, AuthorizationType, CPData, ChargeSession, ChargeSessionReference, EventTypes,
    GetVariableData, MessageReference, ScheduledEvents, EV, EVSE, RFID,
};
use crate::ocpp_1_6::OCPP1_6Client;
use crate::ocpp_2_0_1::OCPP2_0_1Client;
use crate::ocpp_deque::OCPPDeque;
use crate::Client::{OCPP1_6, OCPP2_0_1};
use log::{debug, info, trace};
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
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get connection info from data
        info!("running");

        let mut data = self.data.lock().await;

        let mut evses = HashMap::new();

        for component in &data.variables.components {
            if component.name == "EVSE" {
                let mut meter_energy = 0.0;
                // Try to find MeterEnergy default value from AlignedDataCtrlr
                for aligned_component in &data.variables.components {
                    if aligned_component.name == "AlignedDataCtrlr" && aligned_component.evse.is_none() {
                        for var in &aligned_component.variables {
                            if var.name == "MeterEnergy" {
                                if let Some(ref default_val) = var.default_value {
                                    meter_energy = default_val.to_string().parse().unwrap_or(0.0);
                                }
                                break;
                            }
                        }
                        break;
                    }
                }
                let evse: EVSE = EVSE {
                    is_ac: false,
                    connector_ids: vec![],
                    plugged_in: None,
                    started: false,
                    ev: None,
                    charging_task: None,
                    meter_energy,
                };
                evses.insert(component.evse.unwrap(), evse);
            }
            if component.name == "Connector" {
                if let Some(evse_index) = component.evse {
                    if let Some(connector_id) = component.connector {
                        evses
                            .get_mut(&evse_index)
                            .unwrap()
                            .connector_ids
                            .push(connector_id as u32);
                    }
                }
            }
        }
        data.evses = evses;

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
        trace!("Spawning EVSE task with data");

        let _evse_count = self.data.lock().await.evses.len();
        let cp_data_clone = self.data.clone();
        let comm_arc = self.communicator.clone();

        let cp_data = cp_data_clone.clone();
        let comm = comm_arc.clone();
        tokio::spawn(async move {
            let mut t = 0;
            let comm_arc2 = comm.clone();
            let mut finished_count = 0;
            loop {
                let events = cp_data.lock().await.events.clone();

                if finished_count >= events.len() {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    continue;
                }

                for (idx, event) in events[finished_count..].iter().enumerate() {
                    let i = finished_count + idx;
                    let event_clone = event.clone();
                    tokio::time::sleep(tokio::time::Duration::from_secs(
                        event_clone.duration as u64,
                    ))
                    .await;

                    t += event_clone.duration;
                    info!("Event {} ocurring at time {} {:?}", i, t, event_clone.event);

                    // Clone event data needed for spawned tasks to avoid lifetime issues
                    let event_data = event_clone.event.clone();
                    match event_data {
                        EventTypes::Authorize(auth_type) => {
                            // Try to lock with retry logic
                            loop {
                                match comm_arc2.try_lock() {
                                    Ok(guard) => {
                                        if let Some(comm) = guard.as_ref() {
                                            let _ = comm
                                                .send_authorize(
                                                    auth_type.clone(),
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
                        EventTypes::Plug(reference, ev) => {
                            let mut cp_data_guard = cp_data.lock().await;
                            if let Some(evse) = cp_data_guard.evses.get_mut(&reference.evse_index) {
                                evse.plugged_in = Some(reference.connector_id);
                                evse.ev = Some(ev.clone());
                            }
                            drop(cp_data_guard);
                            CP::check_start_charging(
                                cp_data.clone(),
                                reference.evse_index,
                                comm.clone(),
                            )
                            .await
                        }
                        EventTypes::Unplug(reference) => {
                            info!("Handling unplug event for evse {}", reference.evse_index);
                            CP::check_stop_charging(
                                cp_data.clone(),
                                reference.evse_index,
                                comm.clone(),
                            )
                            .await;
                            let mut cp_data_guard = cp_data.lock().await;
                            if let Some(evse) = cp_data_guard.evses.get_mut(&reference.evse_index) {
                                info!("Handling unplug event for evse {}", reference.evse_index);

                                evse.plugged_in = None;
                                evse.ev = None;
                            }
                        }
                        EventTypes::RemoteStart(reference) => {
                            // Spawn a charging task that updates SOC every second
                            let cp_data_clone = cp_data.clone();
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
                finished_count = events.len();
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
            info!("Stopping the charge charging for evse {}", evse_index);

            if data
                .evses
                .get_mut(&evse_index)
                .unwrap()
                .charging_task
                .is_some()
            {
                data.evses
                    .get_mut(&evse_index)
                    .unwrap()
                    .charging_task
                    .clone()
                    .unwrap()
                    .abort();
                data.evses.get_mut(&evse_index).unwrap().charging_task = None.into();
            }
            drop(data);
            if let Some(_comm) = communicator.lock().await.as_ref() {
                info!("Sending stop for evse {}", evse_index);

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
            drop(data);
            if let Some(comm) = communicator.lock().await.as_ref() {
                let reference: ChargeSessionReference = ChargeSessionReference {
                    evse_index,
                    connector_id: 1, //todo
                };
                info!("Starting charging for evse {}", evse_index);
                let _ = comm.send_start_transaction(reference.clone()).await;
                let cp_data_clone = cp_data.clone();
                let communicator_clone = communicator.clone();
                let charging_task = tokio::spawn(async move {
                    let mut last_meter_send = std::time::Instant::now();
                    let data = cp_data_clone.lock().await;
                    let interval_secs = if let Some(interval_str) = CP::get_variable_value(&data, "MeterValueSampleInterval") {
                        interval_str.parse().unwrap_or(30)
                    } else {
                        300
                    };  
                    drop(data);
                    
                    loop {
                        tokio::time::sleep(Duration::from_secs(1)).await;

                        let mut cp_data_guard = cp_data_clone.lock().await;
                        if let Some(evse) = cp_data_guard.evses.get_mut(&reference.evse_index) {
                            if let Some(ev) = &mut evse.ev {
                                // Lookup power based on SOC using interpolation
                                let power = CP::lookup_power_from_soc(ev);
                                ev.power = power;

                                // Update SOC: add power / capacity * 1s / 3600
                                let soc_increment = (power / ev.capacity) * (1.0 / 3600.0) * 100.0; // convert to percentage
                                ev.soc += soc_increment;

                                // Increment meter energy: power in kW, time interval in seconds
                                // Convert power from kW to Wh: kW * 1s / 3600s = kWh = Wh / 1000
                                evse.meter_energy += (power * 1000.0) * (1.0 / 3600.0); // power in Wh

                                info!("SOC: {:.3}% Power: {:.2}kW Meter: {:.2}Wh", ev.soc, power, evse.meter_energy);
                                drop(cp_data_guard);
                                
                                // Check if it's time to send meter values
                                if last_meter_send.elapsed().as_secs() >= interval_secs {
                                    info!("Sending meter values for evse {}", reference.evse_index);
                                    if let Some(comm) = communicator_clone.lock().await.as_ref() {
                                                                            info!("Sending meter values for evse {}", reference.evse_index);
                                        let _ = comm.send_meter_values(reference.clone()).await;
                                    }
                                    last_meter_send = std::time::Instant::now();
                                }
                            } else {
                                info!(
                                    "EV data missing for evse {}, stopping charging task",
                                    reference.evse_index
                                );
                                break;
                            }
                        }
                    }
                });

                let mut data = cp_data.lock().await;
                data.authorization = None; // Clear authorization after starting charging, for testing purposes
                data.evses.get_mut(&evse_index).unwrap().started = true;

                data.evses.get_mut(&evse_index).unwrap().charging_task =
                    Some(charging_task.abort_handle());
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

    fn get_variable_value(cp_data: &CPData, ocpp16_key: &str) -> Option<String> {
        for component in &cp_data.variables.components {
            for variable in &component.variables {
                if let Some(ref key) = variable.ocpp16_key {
                    if key == ocpp16_key {
                        return variable.default_value.clone().unwrap().to_string().into();
                    }
                }
            }
        }
        None
    }

    fn lookup_power_from_soc(ev: &EV) -> f32 {
        // Find two adjacent points in power_vs_soc where one has lower SOC and one has higher SOC
        if ev.power_vs_soc.is_empty() {
            return 0.0;
        }

        if ev.power_vs_soc.len() == 1 {
            return ev.power_vs_soc[0].1;
        }

        // Find the two adjacent points
        for i in 0..ev.power_vs_soc.len() - 1 {
            let (soc1, power1) = ev.power_vs_soc[i];
            let (soc2, power2) = ev.power_vs_soc[i + 1];

            // Check if current SOC is between these two points
            if ev.soc >= soc1 && ev.soc <= soc2 {
                // Interpolate between power1 and power2
                let soc_diff = soc2 - soc1;
                if soc_diff == 0.0 {
                    return power1;
                }
                let power_diff = power2 - power1;
                let ratio = (ev.soc - soc1) / soc_diff;
                return power1 + (power_diff * ratio);
            }
        }

        // If SOC is outside the range, return the closest power
        if ev.soc <= ev.power_vs_soc[0].0 {
            ev.power_vs_soc[0].1
        } else {
            ev.power_vs_soc[ev.power_vs_soc.len() - 1].1
        }
    }
}
