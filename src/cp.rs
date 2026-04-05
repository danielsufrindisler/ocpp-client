use crate::communicator1_6::OCPPCommunicator1_6;
use crate::communicator2_0_1::OCPPCommunicator2_0_1;
use crate::communicator_trait::OCPPCommunicator;
use crate::cp_data::{
    CPData, ChargeSessionReference, EventInjectionMessage, EventTypes, GetVariableData,
    MessageReference, ScheduledEventWithAbsoluteTime, EV, EVSE,
};
use crate::message_stats::MessageTracker;
use crate::ocpp_1_6::OCPP1_6Client;
use crate::ocpp_2_0_1::OCPP2_0_1Client;
use crate::ocpp_deque::OCPPDeque;
use crate::Client::{OCPP1_6, OCPP2_0_1};
use core::panic;
use log::{debug, info, trace};
use std::collections::{BinaryHeap, HashMap};
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex};

pub struct CP {
    pub communicator: Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>>,
    pub data: Arc<Mutex<CPData>>,
    pub client: Option<crate::Client>,
    pub event_rx: Option<mpsc::Receiver<EventInjectionMessage>>,
    pub message_tracker: Option<Arc<MessageTracker>>,
}

impl CP {
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get connection info from data
        trace!("running 1");

        let mut data = self.data.lock().await;

        let mut evses = HashMap::new();

        for component in &data.variables.components {
            if component.name == "EVSE" {
                let mut meter_energy = 0.0;
                // Try to find MeterEnergy default value from AlignedDataCtrlr
                for aligned_component in &data.variables.components {
                    if aligned_component.name == "AlignedDataCtrlr"
                        && aligned_component.evse.is_none()
                    {
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
                    status: "Available".to_string(),
                    faulted_connectors: vec![],
                    current_tariff: None,
                    idle_tariff: None,
                    running_cost: 0.0,
                    last_cost_update: None,
                    triggers: None,
                    next_period: None,
                    pricing_state: None,
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
        let (trigger_message_requests, rx) = mpsc::channel::<(String, u32)>(100);
        trace!("running 2");

        drop(data);

        // Create client and communicator immediately (unconnected)
        match selected_protocol.as_str() {
            "ocpp1.6" => {
                let client = OCPP1_6Client::new_unconnected(
                    options_username,
                    options_password,
                    address.to_string(),
                    selected_protocol.to_string(),
                    self.message_tracker.clone(),
                    Some(data_clone.lock().await.serial.clone()),
                );
                let _base_clone = client.base.clone();
                *self.communicator.lock().await = Some(Box::new(OCPPCommunicator1_6 {
                    client: client.clone(),
                    data: data_clone.clone(),
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
                    self.message_tracker.clone(),
                    Some(data_clone.lock().await.serial.clone()),
                );
                let _base_clone = client.base.clone();
                *self.communicator.lock().await = Some(Box::new(OCPPCommunicator2_0_1 {
                    client: client.clone(),
                    data: data_clone.clone(),
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
        let cp_data = data_clone;
        trace!("running 3");

        let comm_mpsc = self.communicator.clone();

        comm_mpsc
            .lock()
            .await
            .as_ref()
            .unwrap()
            .register_messages()
            .await;

        info!("Communicator registered messages, starting heartbeat and trigger listeners");

        let heartbeat_data = self.data.clone();
        let heartbeat_comm = self.communicator.clone();
        tokio::spawn(async move {
            loop {
                let interval_secs = {
                    let data = heartbeat_data.lock().await;
                    CP::get_heartbeat_interval(&*data)
                };
                if interval_secs == 0 {
                    // Heartbeat disabled, sleep for a default interval before checking again
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    continue;
                }
                tokio::time::sleep(Duration::from_secs(interval_secs)).await;

                if let Some(comm_lock) = heartbeat_comm.lock().await.as_ref() {
                    if let Err(e) = comm_lock.send_heartbeat().await {
                        log::warn!("Heartbeat send failed: {:?}", e);
                    }
                }
            }
        });

        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(msg) = rx.recv().await {
                let _ = match msg {
                    (ref msg_type, _) if msg_type == "boot_notification" => {
                        debug!("Received boot_notification trigger from mpsc");
                        if let Some(comm) = comm_mpsc.lock().await.as_ref() {
                            let _ = comm.send_boot_notification().await;
                        }
                    }
                    (ref msg_type, evse_index) if msg_type == "remote_start_transaction" => {
                        debug!(
                            "Received remote_start_transaction trigger for evse {} from mpsc",
                            evse_index
                        );
                        CP::check_start_charging(cp_data.clone(), evse_index, comm_mpsc.clone())
                            .await;
                    }
                    _ => debug!("Received unknown trigger {} from mpsc", msg.0),
                };
            }
        });
        trace!("running 4");

        tokio::time::sleep(Duration::from_secs(1)).await;

        // Spawn EVSE tasks immediately
        self.spawn_evse_tasks().await;
        if let Some(comm) = self.communicator.lock().await.as_ref() {
            comm.send_boot_notification().await.ok();
        }


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

    pub fn read_variables_from_file(path: &str) -> GetVariableData {
        let data = fs::read_to_string(path).expect("Unable to read file");
        let mut res: GetVariableData = serde_json::from_str(&data).expect("Unable to parse");

        for component in &mut res.components {
            let mut component_string = component.name.clone();
            if let Some(evse) = component.evse {
                component_string.push_str(evse.to_string().as_str());
            }
            if let Some(instance) = &component.instance {
                component_string.push_str(instance.as_str());
            }

            for variable in &mut component.variables {
                if variable.ocpp16_key.is_none() {
                    variable.ocpp16_key = Some(component_string.clone() + variable.name.as_str());
                }
            }
        }

        res
    }

    pub fn read_cp_config_file(
        path: &str,
    ) -> Result<crate::cp_data::CPConfigFile, Box<dyn std::error::Error + Send + Sync>> {
        let content = fs::read_to_string(path)?;

        // allow old format (direct GetVariableData) and new format with cp/events/components
        if let Ok(config) = serde_json::from_str::<crate::cp_data::CPConfigFile>(&content) {
            return Ok(config);
        }

        let legacy = serde_json::from_str::<GetVariableData>(&content)?;
        Ok(crate::cp_data::CPConfigFile {
            cp: None,
            events: vec![],
            components: legacy.components,
        })
    }

    pub fn save_cp_config_file(
        path: &str,
        config: &crate::cp_data::CPConfigFile,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let json = serde_json::to_string_pretty(config)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn filter_components_with_ocpp16_key(mut data: GetVariableData) -> GetVariableData {
        data.components = data
            .components
            .into_iter()
            .filter_map(|mut component| {
                // Always keep EVSE and Connector components as they are needed for charger configuration
                if component.name == "EVSE" || component.name == "Connector" {
                    component.variables.retain(|v| v.ocpp16_key.is_some());
                    return Some(component);
                }
                // For other components, only keep if they have variables with ocpp16_key
                component.variables.retain(|v| v.ocpp16_key.is_some());
                if component.variables.is_empty() {
                    None
                } else {
                    Some(component)
                }
            })
            .collect();
        data
    }

    pub fn save_variables_to_file(
        path: &str,
        data: &GetVariableData,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let json = serde_json::to_string_pretty(data)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn prepare_cp_variables_files(
        data_directory: &str,
        cp_file_name: &str,
        use_original: bool,
    ) -> Result<
        (crate::cp_data::CPConfigFile, GetVariableData),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let base_path = format!("{}/device_model.json", data_directory);
        let orig_path = format!("{}/{}.orig.json", data_directory, cp_file_name);
        let active_path = format!("{}/{}.json", data_directory, cp_file_name);

        if !std::path::Path::new(&orig_path).exists() {
            // base file might be full component format
            panic!(
                "Original file {} does not exist. Please create it based on the base device model.",
                orig_path
            );
        }

        if use_original || !std::path::Path::new(&active_path).exists() {
            info!(
                "Copying original file {} to active file {}",
                orig_path, active_path
            );
            fs::copy(&orig_path, &active_path)?;
        }

        let mut config = CP::read_cp_config_file(&active_path)?;

        let mut vars: GetVariableData = GetVariableData {
            components: config.components.clone(),
        };

        for component in &mut vars.components {
            let mut component_string = component.name.clone();
            if let Some(evse) = component.evse {
                component_string.push_str(evse.to_string().as_str());
            }
            if let Some(instance) = &component.instance {
                component_string.push_str(instance.as_str());
            }
            for variable in &mut component.variables {
                if variable.ocpp16_key.is_none() {
                    variable.ocpp16_key = Some(component_string.clone() + variable.name.as_str());
                }
            }
        }

        let filtered = CP::filter_components_with_ocpp16_key(vars);
        config.components = filtered.components.clone();
        CP::save_cp_config_file(&active_path, &config)?;

        Ok((config, filtered))
    }

    pub fn get_heartbeat_interval(cp_data: &CPData) -> u64 {
        let candidates = [
            "HeartBeatInterval",
            "HeartbeatInterval",
            "OCPPCommCtrlrHeartbeatInterval",
            "OCPPCommCtrlrHeartbeatInterval", // honour with/without component prefix
        ];
        for key in candidates {
            if let Some(value) = CP::get_variable_value(cp_data, key) {
                if let Ok(v) = value.parse::<u64>() {
                    return v.max(1);
                }
            }
        }
        30
    }

    pub fn set_variable_value(cp_data: &mut CPData, ocpp16_key: &str, new_value: &str) -> bool {
        let mut updated = false;
        for component in &mut cp_data.variables.components {
            debug!(
                "Checking component {} for variable with ocpp16_key {}",
                component.name, ocpp16_key
            );
            for variable in &mut component.variables {
                debug!(
                    "Checking variable {} for variable with ocpp16_key {}",
                    variable.name, ocpp16_key
                );

                if let Some(ref key) = variable.ocpp16_key {
                    debug!(
                        "Checking ocpp16_key {} for variable with ocpp16_key {}",
                        key, ocpp16_key
                    );
                    if key == ocpp16_key {
                        variable.default_value =
                            Some(crate::cp_data::ValueType::String(new_value.to_string()));
                        updated = true;
                        break;
                    }
                }
            }
            if updated {
                break;
            }
        }
        updated
    }

    pub fn persist_variables_to_file(
        cp_data: &CPData,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let active_path = format!("./data/{}.json", cp_data.variables_file_name);
        CP::save_variables_to_file(&active_path, &cp_data.variables)
    }

    pub fn list_components(data: GetVariableData) {
        for component in data.components {
            debug!("Component: {}", component.name);
            if let Some(instance) = component.instance {
                debug!("  Instance: {}", instance);
            }
            if let Some(evse) = component.evse {
                debug!("  EVSE: {}", evse);
            }
            if let Some(connector) = component.connector {
                debug!("  Connector: {}", connector);
            }
        }
    }

    async fn spawn_evse_tasks(&mut self) {
        let event_rx = self.event_rx.take();
        self.spawn_evse_tasks_with_channel(event_rx, 1.0).await;
    }

    /// Spawn EVSE event processing task with optional event injection channel and simulation speed
    pub async fn spawn_evse_tasks_with_channel(
        &self,
        event_rx: Option<mpsc::Receiver<EventInjectionMessage>>,
        speed_multiplier: f64,
    ) {
        trace!("Spawning EVSE task with data");

        let cp_data = self.data.clone();
        let comm_arc = self.communicator.clone();

        tokio::spawn(async move {
            // Convert initial events to priority queue with absolute times
            let mut event_queue: BinaryHeap<ScheduledEventWithAbsoluteTime> = {
                let cp_guard = cp_data.lock().await;
                let now = Instant::now();
                cp_guard
                    .events
                    .iter()
                    .enumerate()
                    .map(|(idx, event)| {
                        let scheduled_time =
                            now + Duration::from_secs_f64(event.duration as f64 / speed_multiplier);
                        ScheduledEventWithAbsoluteTime {
                            scheduled_time,
                            event: event.event.clone(),
                            event_index: idx,
                        }
                    })
                    .collect()
            };

            let mut event_rx = event_rx;
            let mut next_event_index = cp_data.lock().await.events.len();

            loop {
                let now = Instant::now();

                // Process any events that are ready immediately
                while let Some(evt) = event_queue.peek() {
                    if evt.scheduled_time <= now {
                        if let Some(scheduled_evt) = event_queue.pop() {
                            Self::process_event(
                                &cp_data,
                                &comm_arc,
                                scheduled_evt.event,
                                scheduled_evt.event_index,
                            ).await;
                        }
                    } else {
                        break; // Next event is in the future
                    }
                }

                // Get next event from queue
                let next_event = event_queue.peek();
                let sleep_duration = next_event.map(|e| {
                    let duration = e.scheduled_time.saturating_duration_since(now);
                    // Apply speed multiplier: faster simulation = shorter sleep
                    Duration::from_secs_f64(duration.as_secs_f64() * speed_multiplier)
                });

                // Use tokio::select! to wait for either:
                // 1. Timer until next event
                // 2. New event from channel
                tokio::select! {
                    _ = async {
                        match sleep_duration {
                            Some(d) if d.as_millis() > 0 => tokio::time::sleep(d).await,
                            _ => tokio::time::sleep(Duration::from_millis(100)).await,
                        }
                    } => {
                        // Timer fired - process any events that are now ready
                        // (This handles the case where we slept and now events are ready)
                        while let Some(evt) = event_queue.peek() {
                            if evt.scheduled_time <= Instant::now() {
                                if let Some(scheduled_evt) = event_queue.pop() {
                                    Self::process_event(
                                        &cp_data,
                                        &comm_arc,
                                        scheduled_evt.event,
                                        scheduled_evt.event_index,
                                    ).await;
                                }
                            } else {
                                break;
                            }
                        }
                    }
                    msg = async {
                        if let Some(ref mut rx) = event_rx {
                            rx.recv().await
                        } else {
                            std::future::pending().await
                        }
                    } => {
                        match msg {
                            Some(EventInjectionMessage::InsertEvent { event, duration_secs }) => {
                                let scheduled_time = Instant::now() + Duration::from_secs_f64(duration_secs as f64 / speed_multiplier);
                                event_queue.push(ScheduledEventWithAbsoluteTime {
                                    scheduled_time,
                                    event,
                                    event_index: next_event_index,
                                });
                                next_event_index += 1;
                                info!("Injected new event, queue size: {}", event_queue.len());
                            }
                            Some(EventInjectionMessage::Stop) => {
                                info!("Event loop stopped via channel");
                                break;
                            }
                            None => {
                                // Channel closed, continue processing existing events
                                event_rx = None;
                            }
                        }
                    }
                }
            }
        });
    }

    /// Process a single event
    async fn process_event(
        cp_data: &Arc<Mutex<CPData>>,
        comm_arc: &Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>>,
        event_data: EventTypes,
        event_index: usize,
    ) {
        let comm_arc2 = comm_arc.clone();
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
                                        MessageReference::EventIndex(event_index),
                                    )
                                    .await;
                            }
                            break;
                        }
                        Err(_) => {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    }
                }
                
                // For testing purposes, set authorization immediately since we don't have a server
                {
                    let mut cp_data_guard = cp_data.lock().await;
                    cp_data_guard.authorization = Some(auth_type.clone());
                }
                
                // Log current price
                let cp_data_guard = cp_data.lock().await;
                let price_info = CP::get_variable_value(&*cp_data_guard, "DefaultPrice")
                    .unwrap_or("Unknown".to_string());
                info!("Authorize event: current price {}", price_info);
            }
            EventTypes::LocalStop => {
                // todo, if charging, stop charging and transition to finishing
            }
            EventTypes::Plug(reference, ev) => {
                let mut cp_data_guard = cp_data.lock().await;
                let _is_authorized = cp_data_guard.authorization.is_some();
                if let Some(evse) = cp_data_guard.evses.get_mut(&reference.evse_index) {
                    evse.plugged_in = Some(reference.connector_id);
                    evse.ev = Some(ev.clone());
                }
                drop(cp_data_guard);

                // Send StatusNotification
                if let Some(comm_inner) = comm_arc.lock().await.as_ref() {
                    let _ = comm_inner
                        .send_status_notification(1, "Preparing".to_string())
                        .await;
                }

                CP::check_start_charging(cp_data.clone(), reference.evse_index, comm_arc.clone())
                    .await
            }
            EventTypes::Unplug(reference) => {
                info!("Handling unplug event for evse {}", reference.evse_index);
                CP::check_stop_charging(cp_data.clone(), reference.evse_index, comm_arc.clone())
                    .await;
                // Send ConnectorUnplugged DataTransfer
                if let Some(comm) = comm_arc.lock().await.as_ref() {
                    let data = serde_json::json!({
                        "transactionId": reference.evse_index,
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    });
                    let _ = comm
                        .send_data_transfer(
                            "org.openchargealliance.costmsg".to_string(),
                            Some("ConnectorUnplugged".to_string()),
                            Some(data),
                        )
                        .await;
                }
                let mut cp_data_guard = cp_data.lock().await;
                if let Some(evse) = cp_data_guard.evses.get_mut(&reference.evse_index) {
                    info!("Handling unplug event for evse {}", reference.evse_index);
                    evse.plugged_in = None;
                    evse.ev = None;
                    evse.status = "Available".to_string();
                }
                drop(cp_data_guard);

                // Send StatusNotification
                if let Some(comm_inner) = comm_arc.lock().await.as_ref() {
                    let _ = comm_inner
                        .send_status_notification(1, "Available".to_string())
                        .await;
                }
            }
            EventTypes::RemoteStart(_reference) => {
                // Spawn a charging task that updates SOC every second
                let _cp_data_clone = cp_data.clone();
            }
            EventTypes::RemoteStop(_reference) => {
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
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    }
                }
            }
            EventTypes::CommunicationStop => loop {
                match comm_arc2.try_lock() {
                    Ok(mut guard) => {
                        if let Some(comm) = guard.as_mut() {
                            let handle = comm.get_base().handle.clone();
                            if let Some(task_handle) = handle.lock().await.take() {
                                task_handle.abort();
                                comm.get_base().sink.lock().await.take();
                                comm.get_base().stream.lock().await.take();
                                info!("Simulated communication loss: connection aborted");
                            };
                        }
                        break;
                    }
                    Err(_) => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            },
            EventTypes::Faulted(connector_ids) => {
                info!("Handling faulted event for connectors: {:?}", connector_ids);
                let mut cp_data_guard = cp_data.lock().await;
                if let Some(evse) = cp_data_guard.evses.get_mut(&1) { // Default to EVSE 1, can be parameterized
                    if connector_ids.is_empty() {
                        // Fault all connectors
                        evse.faulted_connectors = evse.connector_ids.clone();
                    } else {
                        // Fault specific connectors
                        for &connector_id in &connector_ids {
                            if !evse.faulted_connectors.contains(&connector_id) {
                                evse.faulted_connectors.push(connector_id);
                            }
                        }
                    }
                    evse.status = "Faulted".to_string();
                    info!("Connectors now faulted: {:?}", evse.faulted_connectors);
                }
                drop(cp_data_guard);

                // Send StatusNotification for affected connectors
                if let Some(comm_inner) = comm_arc.lock().await.as_ref() {
                    for connector_id in if connector_ids.is_empty() { 
                        vec![1, 2, 3, 4] // Send for all common connector IDs
                    } else {
                        connector_ids
                    } {
                        let _ = comm_inner
                            .send_status_notification(connector_id, "Faulted".to_string())
                            .await;
                    }
                }
            }
            EventTypes::FaultCleared(connector_ids) => {
                info!("Handling fault cleared event for connectors: {:?}", connector_ids);
                let mut cp_data_guard = cp_data.lock().await;
                if let Some(evse) = cp_data_guard.evses.get_mut(&1) { // Default to EVSE 1
                    if connector_ids.is_empty() {
                        // Clear faults on all connectors
                        evse.faulted_connectors.clear();
                    } else {
                        // Clear faults on specific connectors
                        for connector_id in &connector_ids {
                            evse.faulted_connectors.retain(|&id| id != *connector_id);
                        }
                    }
                    evse.status = if evse.started { "Charging".to_string() } 
                                 else if evse.plugged_in.is_some() { "Preparing".to_string() }
                                 else { "Available".to_string() };
                    info!("Connectors now faulted: {:?}", evse.faulted_connectors);
                }
                drop(cp_data_guard);

                // Send StatusNotification for cleared connectors
                if let Some(comm_inner) = comm_arc.lock().await.as_ref() {
                    for connector_id in if connector_ids.is_empty() {
                        vec![1, 2, 3, 4]
                    } else {
                        connector_ids
                    } {
                        let status = {
                            let data = cp_data.lock().await;
                            if let Some(evse) = data.evses.get(&1) {
                                if evse.started {
                                    "Charging".to_string()
                                } else if evse.plugged_in.is_some() {
                                    "Preparing".to_string()
                                } else {
                                    "Available".to_string()
                                }
                            } else {
                                "Available".to_string()
                            }
                        };
                        let _ = comm_inner
                            .send_status_notification(connector_id, status)
                            .await;
                    }
                }
            }
        }
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
            let connector_id = data.evses[&evse_index].plugged_in.unwrap_or(0);
            info!("Stopping the charge charging for evse {}", evse_index);

            if data
                .evses
                .get_mut(&evse_index)
                .unwrap()
                .charging_task
                .is_some()
            {
                if let Some(task) = data
                    .evses
                    .get_mut(&evse_index)
                    .unwrap()
                    .charging_task
                    .take()
                {
                    task.abort();
                }
            }
            drop(data);
            if let Some(_comm) = communicator.lock().await.as_ref() {
                info!("Sending stop for evse {}", evse_index);

                let reference = ChargeSessionReference {
                    evse_index,
                    connector_id: connector_id, //todo
                };
                let _ = _comm.send_stop_transaction(reference).await;
            }
        }
    }

    async fn check_start_charging(
        cp_data: Arc<Mutex<CPData>>,
        evse_index: u32,
        communicator: Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>>,
    ) {
        debug!("Checking if can start charging for evse {}", evse_index);
        let data = cp_data.lock().await;
        let evse = &data.evses[&evse_index];
        let connector_id = evse.plugged_in.unwrap_or(1);
        let is_faulted = evse.faulted_connectors.is_empty() || evse.faulted_connectors.contains(&connector_id);
        
        debug!("Checking if can start charging for evse {},  Authorized: {}, Plugged in: {:?}, Started: {:?}, Faulted: {}",
            evse_index, data.authorization.is_some(), evse.plugged_in, evse.started, is_faulted);
        if data.authorization.is_some()
            && evse.plugged_in.is_some()
            && !evse.started
            && !is_faulted
        {
            drop(data);
            if let Some(comm) = communicator.lock().await.as_ref() {
                let reference: ChargeSessionReference = ChargeSessionReference {
                    evse_index,
                    connector_id: connector_id,
                };
                info!("Starting charging for evse {}", evse_index);
                let _ = comm.send_start_transaction(reference.clone()).await;
                let _ = comm
                    .send_status_notification(connector_id, "Charging".to_string())
                    .await;
                let cp_data_clone = cp_data.clone();
                let communicator_clone = communicator.clone();
                let charging_task =
                    charging_loop(cp_data_clone, reference.clone(), communicator_clone);

                let mut data = cp_data.lock().await;
                data.authorization = None; // Clear authorization after starting charging, for testing purposes
                data.evses.get_mut(&evse_index).unwrap().started = true;

                data.evses.get_mut(&evse_index).unwrap().charging_task = Some(charging_task);
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

fn charging_loop(
    cp_data: Arc<Mutex<CPData>>,
    reference: ChargeSessionReference,
    communicator: Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_meter_send = std::time::Instant::now();
        let mut previous_power: f64 = 0.0;
        let data = cp_data.lock().await;
        let interval_secs =
            if let Some(interval_str) = CP::get_variable_value(&data, "MeterValueSampleInterval") {
                interval_str.parse().unwrap_or(30)
            } else {
                300
            };
        drop(data);

        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;

            let mut cp_data_guard = cp_data.lock().await;
            let triggers = cp_data_guard.evses[&reference.evse_index].triggers.clone();
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
                    let energy_increment = (power * 1000.0) * (1.0 / 3600.0); // Wh
                    evse.meter_energy += energy_increment;

                    // Calculate running cost
                    if let Some(tariff) = &evse.current_tariff {
                        if let Some(kwh_price) = tariff.kwh_price {
                            let cost_increment = (energy_increment as f64 / 1000.0) * kwh_price; // kWh * $/kWh
                            evse.running_cost += cost_increment;
                        }
                    }

                    let current_price = evse
                        .current_tariff
                        .as_ref()
                        .and_then(|t| t.kwh_price)
                        .unwrap_or(0.0);
                    info!("SOC: {:.3}% Power: {:.2}kW Meter: {:.2}Wh Current Price: {:.3}$/kWh Running Cost: {:.2}$", 
                          ev.soc, power, evse.meter_energy, current_price, evse.running_cost);

                    // Check triggers for extra meter send
                    let mut send_extra_meter = false;
                    if let Some(triggers) = &triggers {
                        // atPowerkW
                        if let Some(threshold) = triggers.at_power_kw {
                            let power_f64 = power as f64;
                            if (previous_power <= threshold && power_f64 > threshold)
                                || (previous_power > threshold && power_f64 <= threshold)
                            {
                                send_extra_meter = true;
                            }
                        }
                        // atEnergykWh
                        if let Some(threshold) = triggers.at_energy_kwh {
                            if (evse.meter_energy as f64 / 1000.0) >= threshold {
                                send_extra_meter = true;
                            }
                        }
                        // atTime
                        if let Some(at_time) = &triggers.at_time {
                            if chrono::Utc::now().timestamp()
                                >= chrono::DateTime::parse_from_rfc3339(at_time)
                                    .unwrap()
                                    .timestamp()
                            {
                                send_extra_meter = true;
                            }
                        }
                    }
                    previous_power = power as f64;

                    // Check if it's time to send meter values
                    if last_meter_send.elapsed().as_secs() >= interval_secs || send_extra_meter {
                        info!("Sending meter values for evse {}", reference.evse_index);
                        drop(cp_data_guard);
                        if let Some(comm) = communicator.lock().await.as_ref() {
                            let _ = comm.send_meter_values(reference.clone()).await;
                        }
                        last_meter_send = std::time::Instant::now();
                    } else {
                        drop(cp_data_guard);
                    }
                } else {
                    drop(cp_data_guard);
                    info!(
                        "EV data missing for evse {}, stopping charging task",
                        reference.evse_index
                    );
                    break;
                }
            }
        }
    })
}
