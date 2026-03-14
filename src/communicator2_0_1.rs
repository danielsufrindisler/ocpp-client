use rust_ocpp::v2_0_1::datatypes::id_token_type::IdTokenType;
use rust_ocpp::v2_0_1::datatypes::transaction_type::TransactionType;
use rust_ocpp::v2_0_1::enumerations::authorization_status_enum_type::AuthorizationStatusEnumType;
use rust_ocpp::v2_0_1::enumerations::id_token_enum_type::IdTokenEnumType;
use rust_ocpp::v2_0_1::enumerations::transaction_event_enum_type::TransactionEventEnumType;
use rust_ocpp::v2_0_1::enumerations::trigger_reason_enum_type::TriggerReasonEnumType;
use rust_ocpp::v2_0_1::messages::authorize::{AuthorizeRequest, AuthorizeResponse};
use rust_ocpp::v2_0_1::messages::boot_notification::BootNotificationRequest as BootNotificationRequest2_0_1;
use rust_ocpp::v2_0_1::messages::transaction_event::TransactionEventRequest;
use rust_ocpp::v2_0_1::messages::meter_values::{MeterValuesRequest, MeterValuesResponse};
use rust_ocpp::v2_0_1::datatypes::meter_value_type::MeterValueType;
use rust_ocpp::v2_0_1::datatypes::sampled_value_type::SampledValueType;

use crate::common_client::CommonOcppClientBase;
use crate::communicator_trait::OCPPCommunicator;
use crate::cp_data::{
    AuthorizationType, CPData, ChargeSessionReference, EventTypes, MessageReference, RFID,
};
use crate::ocpp_2_0_1::OCPP2_0_1Client;
use crate::ocpp_deque::OCPPDeque;
use crate::raw_ocpp_common_call::{RawOcppCommonCall, RawOcppCommonError};
use async_trait::async_trait;
use log::{debug, error, info, log_enabled, trace, warn, Level};
use rust_ocpp::v1_6::messages::trigger_message::TriggerMessageResponse; //todo
use rust_ocpp::v1_6::types::TriggerMessageStatus;
use rust_ocpp::v2_0_1::messages::get_variables::{GetVariablesRequest, GetVariablesResponse};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

pub struct OCPPCommunicator2_0_1 {
    pub client: OCPP2_0_1Client,
    pub data: Arc<Mutex<CPData>>,
    pub trigger_message_requests: Option<mpsc::Sender<String>>,
    pub ocpp_deque: OCPPDeque,
}

#[async_trait]
impl OCPPCommunicator for OCPPCommunicator2_0_1 {
    fn get_base(&mut self) -> &mut CommonOcppClientBase {
        &mut self.client.base
    }

    async fn send_boot_notification(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data: tokio::sync::MutexGuard<'_, CPData> = self.data.lock().await;

        let _ = self
            .client
            .send_boot_notification(BootNotificationRequest2_0_1 {
                charging_station:
                    rust_ocpp::v2_0_1::datatypes::charging_station_type::ChargingStationType {
                        model: data.model.clone(),
                        vendor_name: data.vendor.clone(),
                        serial_number: Some(data.serial.clone()),
                        ..Default::default()
                    },
                ..Default::default()
            })
            .await;
        Ok(())
    }

    async fn send_authorize(
        &self,
        authorization_data: AuthorizationType,
        message_reference: MessageReference,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let authorize_request: AuthorizeRequest;
        match &authorization_data {
            AuthorizationType::RFID(rfid) => {
                debug!("Sending authorize for RFID: {}", rfid.id_tag);
                authorize_request = AuthorizeRequest {
                    id_token: IdTokenType {
                        id_token: rfid.id_tag.clone(),
                        kind: IdTokenEnumType::ISO15693,
                        additional_info: None,
                    },
                    ..Default::default()
                };
            }
            AuthorizationType::PlugAndCharge(_pnc) => {
                debug!("Sending authorize for PlugAndCharge");
                authorize_request = AuthorizeRequest {
                    id_token: IdTokenType {
                        id_token: "PlugAndChargeToken".to_string(), //TODO generate proper token
                        kind: IdTokenEnumType::EMAID,
                        additional_info: None,
                    },
                    ..Default::default()
                };
            }
            AuthorizationType::Remote(rfid) => {
                debug!("Sending authorize for Remote: {}", rfid.id_tag);
                authorize_request = AuthorizeRequest {
                    id_token: IdTokenType {
                        id_token: rfid.id_tag.clone(),
                        kind: IdTokenEnumType::ISO15693,
                        additional_info: None,
                    },
                    ..Default::default()
                };
            }
        }

        //TODO Some Reference
        let response = self
            .ocpp_deque
            .do_send_request_queued(authorize_request, "Authorize", Some(message_reference))
            .await;

        //  let response = self.ocpp_deque.do_send_request_raw::<BootNotificationResponse>(message_id, call, "BootNotification").await;

        if let Ok(_) = response {
            debug!("Authorize is schedule");
        } else {
            error!("Authorize failed to schedule");
        }

        let cp_data_arc = Arc::clone(&self.data);
        let callback = move |call: RawOcppCommonCall,
                             ocpp_result: Result<Value, RawOcppCommonError>,
                             reference: Option<MessageReference>,
                             _self_clone: OCPPDeque| {
            trace!("Authorize callback invoked");
            let cp_data_arc2 = Arc::clone(&cp_data_arc);
            async move {
                trace!("Authorize callback invoked1");
                let authorize_response: AuthorizeResponse =
                    serde_json::from_value(ocpp_result.unwrap()).unwrap(); //todo handle error
                trace!(
                    "Authorize response received for message ID {}: {:?}",
                    call.1,
                    authorize_response
                );

                let mut lock = cp_data_arc2.lock().await;
                if authorize_response.id_token_info.status == AuthorizationStatusEnumType::Accepted
                {
                    if let Some(MessageReference::EventIndex(event_index)) = reference {
                        info!("Authorization accepted for ID tag");
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
                        info!(
                            "Authorization rejected for ID tag for evse {}",
                            cs_ref.evse_index
                        );
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

        Ok(())
    }

    async fn register_messages(&self) -> () {
        let _ = self.register_trigger_message().await;
        /*

                let data = self.data.clone();
                let callback = move | request: GetVariablesRequest, _client: OCPP2_0_1Client| {
                    let _data = data.clone();
                    async move {
                        debug!("Received GetVariables from server");

                        if request.get_variable_data
                        if request.key.is_none() {
                            debug!("GetVariables request with empty keys, returning all variables");
                                 Ok(GetVariablesResponse {
                            configuration_key: None,
                            unknown_key: None,
                        })
                    }
                         else {
                            debug!("GetVariables request for keys: {:?}", request.key);
                             Ok(GetVariablesResponse {
                            configuration_key: None,
                            unknown_key: Some(request.key.unwrap()),
                        })
                        }
                    }

                };
                info!("Registering GetConfiguration callback");
                self.client.on_get_configuration(callback).await;

        */
    }

    async fn register_trigger_message(
        &self,
    ) -> Result<TriggerMessageResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(TriggerMessageResponse {
            status: TriggerMessageStatus::Accepted,
        })
    }

    async fn send_start_transaction(
        &self,
        reference: ChargeSessionReference,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data = self.data.lock().await;
        let evse = &data.evses[&reference.evse_index];

        let id_tag;

        if let Some(authorization) = &data.authorization {
            match authorization {
                AuthorizationType::RFID(rfid) => {
                    debug!("Using RFID {} for StartTransaction", rfid.id_tag);
                    id_tag = rfid.id_tag.clone();
                }
                AuthorizationType::Remote(rfid) => {
                    debug!("Using RFID {} for StartTransaction", rfid.id_tag);
                    id_tag = rfid.id_tag.clone();
                }
                AuthorizationType::PlugAndCharge(_pnc) => {
                    debug!("Using PlugAndCharge for StartTransaction");
                    id_tag = "PlugAndChargeToken".to_string(); //TODO generate proper token
                }
            }
        }

        let start_transaction_request = TransactionEventRequest {
            event_type: TransactionEventEnumType::Started,
            timestamp: chrono::Utc::now(),
            trigger_reason: TriggerReasonEnumType::CablePluggedIn, //todo determine proper trigger reason
            seq_no: 1, //todo generate proper sequence number
            transaction_info: TransactionType {
                transaction_id: "123".to_string(), //TODO generate proper transaction ID
                ..Default::default()
            },
            ..Default::default()
        };

        let response = self
            .ocpp_deque
            .do_send_request_queued(
                start_transaction_request,
                "TransactionEvent",
                Some(MessageReference::ChargeSession(reference)),
            )
            .await;

        if let Ok(_) = response {
            trace!("StartTransaction is scheduled");
        } else {
            error!("StartTransaction failed to schedule");
        }

        Ok(())
    }

    async fn send_meter_values(
        &self,
        reference: ChargeSessionReference,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        debug!(
            "Preparing to send MeterValues for evse {}",
            reference.evse_index
        );
        let data = self.data.lock().await;

        let evse = &data.evses[&reference.evse_index];

        // Get Measurands from device model
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

        // Create sampled values
        let _sampled_values: Vec<SampledValueType> = Vec::new();
        
        for measurand in measurands {
            let _value = match measurand {
                "Energy.Active.Import.Register" => evse.meter_energy,
                "Power.Active.Import" => {
                    if let Some(ev) = &evse.ev {
                        ev.power * 1000.0 // Convert kW to W
                    } else {
                        0.0
                    }
                },
                "Current.Import" => {
                    // Calculate approximate current assuming 230V per phase
                    if let Some(ev) = &evse.ev {
                        ev.power * 1000.0 / 230.0 // Current in A
                    } else {
                        0.0
                    }
                },
                _ => 0.0,
            };

            info!("Meter value for {}: {}", measurand, _value);
        }

        // For now, send empty meter values as placeholder
        // TODO: Properly construct sampled_values with correct SampledValueType
        let meter_values_request = MeterValuesRequest {
            evse_id: reference.evse_index as i32,
            meter_value: vec![MeterValueType {
                timestamp: chrono::Utc::now(),
                sampled_value: vec![],
            }],
        };

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
}
