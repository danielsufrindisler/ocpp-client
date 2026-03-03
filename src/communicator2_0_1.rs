use rust_ocpp::v2_0_1::datatypes::id_token_type::IdTokenType;
use rust_ocpp::v2_0_1::datatypes::transaction_type::TransactionType;
use rust_ocpp::v2_0_1::enumerations::authorization_status_enum_type::AuthorizationStatusEnumType;
use rust_ocpp::v2_0_1::enumerations::id_token_enum_type::IdTokenEnumType;
use rust_ocpp::v2_0_1::enumerations::transaction_event_enum_type::TransactionEventEnumType;
use rust_ocpp::v2_0_1::enumerations::trigger_reason_enum_type::TriggerReasonEnumType;
use rust_ocpp::v2_0_1::messages::authorize::{AuthorizeRequest, AuthorizeResponse};
use rust_ocpp::v2_0_1::messages::boot_notification::BootNotificationRequest as BootNotificationRequest2_0_1;
use rust_ocpp::v2_0_1::messages::transaction_event::TransactionEventRequest;

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
use rust_ocpp::v1_6::messages::trigger_message::TriggerMessageResponse;
use rust_ocpp::v1_6::types::TriggerMessageStatus;
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
                            "Authorization rejected for ID tag for evse {}, session {}",
                            cs_ref.evse_index, cs_ref.charge_session_index
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
        let evse = &data.evses[reference.evse_index];
        let charge_session = &evse.charge_sessions[reference.charge_session_index];

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
}
