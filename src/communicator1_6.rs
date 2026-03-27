use rust_ocpp::v1_6::messages::authorize::{AuthorizeRequest, AuthorizeResponse};
use rust_ocpp::v1_6::messages::boot_notification::BootNotificationRequest;
use rust_ocpp::v1_6::messages::change_configuration::{
    ChangeConfigurationRequest, ChangeConfigurationResponse,
};
use rust_ocpp::v1_6::messages::data_transfer::{DataTransferRequest, DataTransferResponse};
use rust_ocpp::v1_6::messages::meter_values::MeterValuesRequest;
use rust_ocpp::v1_6::messages::remote_start_transaction::{
    RemoteStartTransactionRequest, RemoteStartTransactionResponse,
};
use rust_ocpp::v1_6::messages::remote_stop_transaction::{
    RemoteStopTransactionRequest, RemoteStopTransactionResponse,
};
use rust_ocpp::v1_6::messages::start_transaction::StartTransactionRequest;
use rust_ocpp::v1_6::messages::stop_transaction::StopTransactionRequest;
use rust_ocpp::v1_6::types::ConfigurationStatus;
use rust_ocpp::v1_6::types::KeyValue;
use rust_ocpp::v1_6::types::Reason;
use rust_ocpp::v1_6::types::RemoteStartStopStatus;
use rust_ocpp::v1_6::types::{Measurand, SampledValue};

use crate::common_client::CommonOcppClientBase;
use crate::communicator_trait::OCPPCommunicator;
use crate::cp::CP;
use crate::cp_data::{
    AuthorizationType, CPData, ChargeSessionReference, EventTypes, FinalCostData, MessageReference,
    RunningCostData, SetUserPriceData,
};
use crate::ocpp_1_6::OCPP1_6Client;
use crate::ocpp_deque::OCPPDeque;
use crate::raw_ocpp_common_call::{RawOcppCommonCall, RawOcppCommonError};
use async_trait::async_trait;
use chrono;
use log::{debug, error, info, trace, warn};
use rust_ocpp::v1_6::messages::get_configuration::{
    GetConfigurationRequest, GetConfigurationResponse,
};
use rust_ocpp::v1_6::messages::trigger_message::{TriggerMessageRequest, TriggerMessageResponse};

use rust_ocpp::v1_6::types::TriggerMessageStatus;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

pub struct OCPPCommunicator1_6 {
    pub client: OCPP1_6Client,
    pub data: Arc<Mutex<CPData>>,
    pub trigger_message_requests: Option<mpsc::Sender<(String, u32)>>,
    pub ocpp_deque: OCPPDeque,
}

#[async_trait]
impl OCPPCommunicator for OCPPCommunicator1_6 {
    fn get_base(&mut self) -> &mut CommonOcppClientBase {
        &mut self.client.base
    }

    async fn send_boot_notification(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data: tokio::sync::MutexGuard<'_, CPData> = self.data.lock().await;
        let boot_notification: BootNotificationRequest = BootNotificationRequest {
            charge_point_model: data.model.clone(),
            charge_point_vendor: data.vendor.clone(),
            charge_point_serial_number: Some(data.serial.clone()),
            ..Default::default()
        };
        let _message_id = Uuid::new_v4();

        let cp_data_arc = Arc::clone(&self.data);
        let callback = move |call: RawOcppCommonCall,
                             ocpp_result: Result<Value, RawOcppCommonError>,
                             _: Option<MessageReference>,
                             _self_clone: OCPPDeque| {
            debug!("BootNotification callback invoked");
            let cp_data_arc = cp_data_arc.clone();
            async move {
                trace!("BootNotification callback invoked1");

                info!(
                    "BootNotification response received for message ID {}: {:?}",
                    call.1, ocpp_result
                );
                let mut lock = cp_data_arc.lock().await;
                lock.booted = true;
                trace!("Booted set to true");
            }
        };

        self.ocpp_deque
            .handle_on_response(callback, "BootNotification")
            .await;

        let response = self
            .ocpp_deque
            .do_send_request_queued(boot_notification, "BootNotification", None)
            .await;

        //  let response = self.ocpp_deque.do_send_request_raw::<BootNotificationResponse>(message_id, call, "BootNotification").await;

        if let Ok(_) = response {
            trace!("Boot is scheduled");
        }
        Ok(())
    }

    async fn send_authorize(
        &self,
        authorization_data: AuthorizationType,
        message_reference: MessageReference,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let AuthorizationType::RFID(rfid) = &authorization_data {
            trace!("Sending authorize for RFID: {}", rfid.id_tag);
            let authorize_request = AuthorizeRequest {
                id_tag: rfid.id_tag.clone(),
            };
            let _id_tag2 = rfid.id_tag.clone();
            //TODO Some Reference
            let response = self
                .ocpp_deque
                .do_send_request_queued(authorize_request, "Authorize", Some(message_reference))
                .await;

            //  let response = self.ocpp_deque.do_send_request_raw::<BootNotificationResponse>(message_id, call, "BootNotification").await;

            if let Ok(_) = response {
                trace!("Authorize is schedule");
            } else {
                warn!("Authorize failed to schedule");
            }

            let cp_data_arc = Arc::clone(&self.data);
            let callback = move |call: RawOcppCommonCall,
                                 ocpp_result: Result<Value, RawOcppCommonError>,
                                 reference: Option<MessageReference>,
                                 _self_clone: OCPPDeque| {
                trace!("Authorize callback invoked");
                let cp_data_arc = cp_data_arc.clone();
                async move {
                    trace!("Authorize callback invoked1");
                    let authorize_response: AuthorizeResponse =
                        serde_json::from_value(ocpp_result.unwrap()).unwrap(); //todo handle error
                    trace!(
                        "Authorize response received for message ID {}: {:?}",
                        call.1,
                        authorize_response
                    );

                    let mut lock = cp_data_arc.lock().await;
                    if authorize_response.id_tag_info.status
                        == crate::rust_ocpp::v1_6::types::AuthorizationStatus::Accepted
                    {
                        if let Some(MessageReference::EventIndex(event_index)) = reference {
                            info!("Authorization accepted");
                            match &lock.events[event_index].event {
                                EventTypes::Authorize(authorization_data) => {
                                    lock.authorization = Some(authorization_data.clone());
                                }
                                _ => {
                                    warn!("Unexpected event type for authorization response");
                                }
                            }
                        }
                    } else {
                        if let Some(MessageReference::ChargeSession(cs_ref)) = reference {
                            info!("Authorization re for ID tag for evse {}", cs_ref.evse_index);
                            //TODO, above not needed
                        } else {
                            warn!("Unexpected bahaviour, authorization failed but no charge session reference provided");
                        }
                    }
                }
            };

            self.ocpp_deque
                .handle_on_response(callback, "Authorize")
                .await;
        } else {
            warn!("Cannot send authorize for ocpp1_6 only RFID is supported");
        }

        Ok(())
    }

    async fn register_messages(&self) -> () {
        let _ = self.register_trigger_message().await;

        let data = self.data.clone();
        let callback = move |request: GetConfigurationRequest, _client: OCPP1_6Client| {
            let _data = data.clone();
            async move {
                debug!("Received GetConfiguration from server");

                if request.key.is_none() {
                    debug!("GetConfiguration request with empty keys, returning all configuration");
                    Ok(GetConfigurationResponse {
                        configuration_key: None,
                        unknown_key: None,
                    })
                } else {
                    debug!("GetConfiguration request for keys: {:?}", request.key);
                    let mut unknown_keys = vec![];
                    let mut configuration_keys: Vec<KeyValue> = vec![];
                    for key in request.key.clone().unwrap() {
                        let mut found = false;
                        for component in &_data.lock().await.variables.components {
                            for variable in &component.variables {
                                if variable.ocpp16_key.clone().unwrap_or("".to_string()) == key {
                                    configuration_keys.push(KeyValue {
                                        key: key.clone(),
                                        value: Some(
                                            variable.default_value.as_ref().unwrap().to_string(),
                                        ),
                                        readonly: variable.attributes.mutability == "ReadOnly",
                                    });
                                    found = true;
                                    break;
                                }
                            }
                        }
                        if !found {
                            unknown_keys.push(key.clone());
                        }
                    }
                    let unkown_keys = if unknown_keys.is_empty() {
                        None
                    } else {
                        Some(unknown_keys)
                    };
                    let configuration_keys = if configuration_keys.is_empty() {
                        None
                    } else {
                        Some(configuration_keys)
                    };
                    Ok(GetConfigurationResponse {
                        configuration_key: configuration_keys,
                        unknown_key: unkown_keys,
                    })
                }
            }
        };
        info!("Registering GetConfiguration callback");
        self.client.on_get_configuration(callback).await;

        // Register ChangeConfiguration
        let data = self.data.clone();
        let callback = move |request: ChangeConfigurationRequest, _client: OCPP1_6Client| {
            let data = data.clone();
            async move {
                debug!(
                    "Received ChangeConfiguration request: key={}, value={}",
                    request.key, request.value
                );
                let mut data_guard = data.lock().await;
                let status =
                    if CP::set_variable_value(&mut data_guard, &request.key, &request.value) {
                        if let Err(e) = CP::persist_variables_to_file(&data_guard) {
                            error!("Failed to persist configuration file: {:?}", e);
                        }
                        ConfigurationStatus::Accepted
                    } else {
                        ConfigurationStatus::NotSupported
                    };

                Ok(ChangeConfigurationResponse { status })
            }
        };
        info!("Registering ChangeConfiguration callback");
        self.client.on_change_configuration(callback).await;

        let internal_producer = self.trigger_message_requests.clone();

        // Register RemoteStartTransaction
        let data = self.data.clone();
        let callback = move |request: RemoteStartTransactionRequest, _client: OCPP1_6Client| {
            let data = data.clone();
            let internal_producer = internal_producer.clone();
            async move {
                debug!(
                    "Received RemoteStartTransaction: connector_id={:?}, id_tag={}",
                    request.connector_id, request.id_tag
                );
                let mut evse_index: Option<u32> = None;
                let data_guard = data.lock().await;
                if let Some(connector) = request.connector_id {
                    for (&idx, evse) in &data_guard.evses {
                        if evse.connector_ids.contains(&connector) {
                            evse_index = Some(idx);
                            break;
                        }
                    }
                } else {
                    if data_guard.evses.len() == 1 {
                        for (&idx, evse) in &data_guard.evses {
                            if evse.status != "Charging" {
                                evse_index = Some(idx);
                            }
                        }
                    } else {
                        let mut plugged_not_charging = vec![];
                        for (&idx, evse) in &data_guard.evses {
                            if evse.plugged_in.is_some() && evse.status != "Charging" {
                                plugged_not_charging.push(idx);
                            }
                        }
                        if plugged_not_charging.len() == 1 {
                            evse_index = Some(plugged_not_charging[0]);
                        }
                    }
                }
                drop(data_guard);
                if let Some(evse_idx) = evse_index {
                    let mut data_guard = data.lock().await;
                    data_guard.authorization =
                        Some(AuthorizationType::Remote(crate::cp_data::RFID {
                            id_tag: (request.id_tag.clone()),
                        }));
                    internal_producer
                        .as_ref()
                        .unwrap()
                        .send(("remote_start_transaction".to_string(), evse_idx))
                        .await
                        .unwrap();
                    Ok(RemoteStartTransactionResponse {
                        status: RemoteStartStopStatus::Accepted,
                    })
                } else {
                    warn!(
                        "No evse found for RemoteStartTransaction with connector_id {:?}",
                        request.connector_id
                    );
                    Ok(RemoteStartTransactionResponse {
                        status: RemoteStartStopStatus::Rejected,
                    })
                }
            }
        };
        info!("Registering RemoteStartTransaction callback");
        self.client.on_remote_start_transaction(callback).await;

        // Register RemoteStopTransaction
        let data = self.data.clone();
        let callback = move |request: RemoteStopTransactionRequest, _client: OCPP1_6Client| {
            let data = data.clone();
            async move {
                debug!(
                    "Received RemoteStopTransaction: transaction_id={}",
                    request.transaction_id
                );
                // Find the charging evse
                let evse_index = {
                    let data_guard = data.lock().await;
                    data_guard.evses.iter().find_map(|(idx, evse)| {
                        if evse.status == "Charging" {
                            Some(*idx)
                        } else {
                            None
                        }
                    })
                };
                if let Some(evse_idx) = evse_index {
                    let mut data_guard = data.lock().await;
                    // Add RemoteStop event
                    Ok(RemoteStopTransactionResponse {
                        status: RemoteStartStopStatus::Accepted,
                    })
                } else {
                    warn!("No charging evse found for RemoteStop");
                    Ok(RemoteStopTransactionResponse {
                        status: RemoteStartStopStatus::Rejected,
                    })
                }
            }
        };
        info!("Registering RemoteStopTransaction callback");
        self.client.on_remote_stop_transaction(callback).await;

        // Register DataTransfer for pricing messages
        let data = self.data.clone();
        let callback = move |request: DataTransferRequest, _client: OCPP1_6Client| {
            let data = data.clone();
            async move {
                debug!(
                    "Received DataTransfer: vendor_string={}, message_id={:?}",
                    request.vendor_string, request.message_id
                );
                if request.vendor_string == "org.openchargealliance.costmsg" {
                    if let Some(message_id) = &request.message_id {
                        match message_id.as_str() {
                            "SetUserPrice" => {
                                // Handle SetUserPrice
                                if let Some(data_str) = &request.data {
                                    if let Ok(value) = serde_json::from_str::<Value>(data_str) {
                                        if let Ok(set_price) =
                                            serde_json::from_value::<SetUserPriceData>(value)
                                        {
                                            info!(
                                                "Received SetUserPrice for id_token: {}",
                                                set_price.id_token
                                            );
                                            // TODO: Store user-specific price
                                        }
                                    }
                                }
                            }
                            "RunningCost" => {
                                // Handle RunningCost
                                if let Some(data_str) = &request.data {
                                    if let Ok(value) = serde_json::from_str::<Value>(data_str) {
                                        if let Ok(running_cost) =
                                            serde_json::from_value::<RunningCostData>(value)
                                        {
                                            info!(
                                                "Received RunningCost for transaction {}: cost={}",
                                                running_cost.transaction_id, running_cost.cost
                                            );
                                            // TODO: Update EVSE pricing state
                                        }
                                    }
                                }
                            }
                            "FinalCost" => {
                                // Handle FinalCost
                                if let Some(data_str) = &request.data {
                                    if let Ok(value) = serde_json::from_str::<Value>(data_str) {
                                        if let Ok(final_cost) =
                                            serde_json::from_value::<FinalCostData>(value)
                                        {
                                            info!("Received FinalCost for transaction {}: cost={}, text={}", final_cost.transaction_id, final_cost.cost, final_cost.price_text);
                                            // TODO: Display final bill
                                        }
                                    }
                                }
                            }
                            _ => {
                                debug!("Unknown message_id: {}", message_id);
                            }
                        }
                    }
                }
                Ok(DataTransferResponse {
                    status: rust_ocpp::v1_6::types::DataTransferStatus::Accepted,
                    data: None,
                })
            }
        };
        info!("Registering DataTransfer callback");
        self.client.on_data_transfer(callback).await;
    }

    async fn send_heartbeat(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let response = self
            .client
            .send_heartbeat(rust_ocpp::v1_6::messages::heart_beat::HeartbeatRequest {
                ..Default::default()
            })
            .await;

        match response {
            Ok(_) => {
                info!("Heartbeat request sent");
                Ok(())
            }
            Err(e) => {
                error!("Heartbeat request failed: {:?}", e);
                Err(e)
            }
        }
    }

    async fn register_trigger_message(
        &self,
    ) -> Result<TriggerMessageResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Register TriggerMessage callback for OCPP 1.6
        let data = self.data.clone();
        let trigger_message_requests = self.trigger_message_requests.clone();
        let callback = move |_request: TriggerMessageRequest, _client: OCPP1_6Client| {
            let _data = data.clone();
            let trigger_message_requests = trigger_message_requests.clone();
            async move {
                debug!("Received TriggerMessage from server");
                if let Some(sender) = trigger_message_requests {
                    let _ = sender.send(("boot_notification".to_string(), 0)).await;
                }
                Ok(TriggerMessageResponse {
                    status: TriggerMessageStatus::Accepted,
                })
            }
        };
        self.client.on_trigger_message(callback).await;
        Ok(TriggerMessageResponse {
            status: TriggerMessageStatus::Accepted,
        })
    }

    async fn send_start_transaction(
        &self,
        reference: ChargeSessionReference,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        debug!(
            "Preparing to send StartTransaction for evse {}",
            reference.evse_index
        );
        let data = self.data.lock().await;
        debug!(
            "Preparing to send StartTransaction for evse {}",
            reference.evse_index,
        );

        let evse = &data.evses[&reference.evse_index];

        let id_tag;

        if let Some(AuthorizationType::RFID(rfid)) = &data.authorization {
            debug!("Using RFID {} for StartTransaction", rfid.id_tag);
            id_tag = rfid.id_tag.clone();
        } else if let Some(AuthorizationType::Remote(rfid)) = &data.authorization {
            debug!("Using RFID {} for StartTransaction", rfid.id_tag);
            id_tag = rfid.id_tag.clone();
        } else {
            return Err("StartTransaction requires RFID authorization".into());
        }

        let start_transaction_request = StartTransactionRequest {
            connector_id: evse.connector_ids[0], // Assuming first connector
            id_tag,
            meter_start: 0, // Placeholder, should be actual meter value
            timestamp: chrono::Utc::now(),
            reservation_id: None,
            ..Default::default()
        };

        let response = self
            .ocpp_deque
            .do_send_request_queued(
                start_transaction_request,
                "StartTransaction",
                Some(MessageReference::ChargeSession(reference)),
            )
            .await;

        if let Ok(_) = response {
            trace!("StartTransaction is scheduled");
        } else {
            trace!("StartTransaction failed to schedule");
        }

        // TODO: Add callback for StartTransaction response
        // let callback = ...;

        // self.ocpp_deque
        //     .handle_on_response(callback, "StartTransaction")
        //     .await;

        Ok(())
    }

    async fn send_stop_transaction(
        &self,
        reference: ChargeSessionReference,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        debug!(
            "Preparing to send StopTransaction for evse {}",
            reference.evse_index
        );
        let data = self.data.lock().await;

        let evse = &data.evses[&reference.evse_index];

        let stop_transaction_request = StopTransactionRequest {
            meter_stop: (evse.meter_energy * 1000.0) as i32, // Convert Wh to Wh
            timestamp: chrono::Utc::now(),
            transaction_id: 123, // TODO: Store actual transaction ID
            reason: Some(Reason::Local),
            id_tag: None,
            ..Default::default()
        };

        let response = self
            .ocpp_deque
            .do_send_request_queued(
                stop_transaction_request,
                "StopTransaction",
                Some(MessageReference::ChargeSession(reference)),
            )
            .await;

        if let Ok(_) = response {
            trace!("StopTransaction is scheduled");
        } else {
            error!("StopTransaction failed to schedule");
        }

        // TODO: Add callback for StopTransaction response
        // let callback = ...;

        // self.ocpp_deque
        //     .handle_on_response(callback, "StopTransaction")
        //     .await;

        Ok(())
    }

    async fn send_meter_values(
        &self,
        reference: ChargeSessionReference,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Preparing to send MeterValues for evse {}",
            reference.evse_index
        );
        let data = self.data.lock().await;

        let evse = &data.evses[&reference.evse_index];
        info!(
            "Preparing to send MeterValues for evse {}",
            reference.evse_index
        );
        // Get Measurands and Interval from device model
        let mut measurands_str = String::new();

        for component in &data.variables.components {
            if component.name == "AlignedDataCtrlr" && component.evse.is_none() {
                for var in &component.variables {
                    if var.name == "Measurands" {
                        if let Some(ref default_val) = var.default_value {
                            measurands_str = default_val.to_string();
                        }
                    }
                }
            }
        }

        // Parse measurands from comma-separated list
        let measurands: Vec<&str> = measurands_str.split(',').map(|s| s.trim()).collect();

        // Create meter value samples
        let mut sampled_values: Vec<SampledValue> = Vec::new();
        for measurand_string in measurands {
            let (measurand, value): (Measurand, String) = match measurand_string {
                "Energy.Active.Import.Register" => (
                    Measurand::EnergyActiveImportRegister,
                    evse.meter_energy.to_string(),
                ),
                "Power.Active.Import" => (
                    Measurand::PowerActiveImport,
                    if let Some(ev) = &evse.ev {
                        (ev.power * 1000.0).to_string() // Convert kW to W
                    } else {
                        "0".to_string()
                    },
                ),
                "Current.Import" => (
                    Measurand::CurrentImport,
                    if let Some(ev) = &evse.ev {
                        (ev.power * 1000.0 / 230.0).to_string() // Current in A
                    } else {
                        "0".to_string()
                    },
                ),
                _ => {
                    warn!("Unsupported measurand: {}", measurand_string);
                    continue;
                }
            };

            info!("Meter value for {:?}: {}", measurand, value);
            sampled_values.push(SampledValue {
                value,
                context: None,
                format: None,
                measurand: Some(measurand),
                phase: None,
                location: None,
                unit: None,
            });
        }

        // For now, send empty meter values as placeholder
        // TODO: Properly construct meter_values with SampledValue type
        let meter_values_request = MeterValuesRequest {
            connector_id: evse.connector_ids.first().copied().unwrap_or(1),
            meter_value: vec![rust_ocpp::v1_6::types::MeterValue {
                timestamp: chrono::Utc::now(),
                sampled_value: sampled_values,
            }],
            transaction_id: None,
        };
        info!(
            "Preparing to send MeterValues for evse {}",
            reference.evse_index
        );
        let response = self
            .ocpp_deque
            .do_send_request_queued(
                meter_values_request,
                "MeterValues",
                Some(MessageReference::ChargeSession(reference)),
            )
            .await;

        if let Ok(_) = response {
            trace!("MeterValues is scheduled");
        } else {
            trace!("MeterValues failed to schedule");
        }

        Ok(())
    }

    async fn send_status_notification(
        &self,
        connector_id: u32,
        status: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use rust_ocpp::v1_6::types::ChargePointErrorCode;
        use rust_ocpp::v1_6::types::ChargePointStatus;

        let status_enum = match status.as_str() {
            "Available" => ChargePointStatus::Available,
            "Preparing" => ChargePointStatus::Preparing,
            "Charging" => ChargePointStatus::Charging,
            "SuspendedEVSE" => ChargePointStatus::SuspendedEVSE,
            "SuspendedEV" => ChargePointStatus::SuspendedEV,
            "Finishing" => ChargePointStatus::Finishing,
            "Reserved" => ChargePointStatus::Reserved,
            "Unavailable" => ChargePointStatus::Unavailable,
            "Faulted" => ChargePointStatus::Faulted,
            _ => ChargePointStatus::Available,
        };

        let status_notification_request =
            rust_ocpp::v1_6::messages::status_notification::StatusNotificationRequest {
                connector_id,
                status: status_enum,
                error_code: ChargePointErrorCode::NoError, // TODO: proper error code
                info: None,
                timestamp: Some(chrono::Utc::now()),
                vendor_id: None,
                vendor_error_code: None,
            };

        let response = self
            .ocpp_deque
            .do_send_request_queued(
                status_notification_request,
                "StatusNotification",
                Some(MessageReference::NoReference),
            )
            .await;

        if let Ok(_) = response {
            trace!("StatusNotification is scheduled");
        } else {
            trace!("StatusNotification failed to schedule");
        }

        Ok(())
    }

    async fn send_data_transfer(
        &self,
        vendor_id: String,
        message_id: Option<String>,
        data: Option<Value>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data_str = data.map(|v| v.to_string());
        let request = DataTransferRequest {
            vendor_string: vendor_id,
            message_id,
            data: data_str,
        };

        let response = self
            .ocpp_deque
            .do_send_request_queued(request, "DataTransfer", Some(MessageReference::NoReference))
            .await;

        if let Ok(_) = response {
            trace!("DataTransfer is scheduled");
        } else {
            trace!("DataTransfer failed to schedule");
        }

        Ok(())
    }
}
