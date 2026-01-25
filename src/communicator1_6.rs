use crate::Client::{OCPP1_6, OCPP2_0_1};
use crate::{connect, ConnectOptions};
use rust_ocpp::v1_6::messages::authorize::{AuthorizeRequest, AuthorizeResponse};
use rust_ocpp::v1_6::messages::boot_notification::{
    BootNotificationRequest, BootNotificationResponse,
};
use rust_ocpp::v1_6::messages::status_notification::{
    StatusNotificationRequest, StatusNotificationResponse,
};

use crate::communicator_trait::OCPPCommunicator;
use crate::cp_data::{
    AuthorizationType, CPData, ChargeSession, ChargeSessionReference, MessageReference,
    PlugAndCharge, EV, EVSE, RFID,
};
use crate::ocpp_1_6::OCPP1_6Client;
use crate::ocpp_deque::OCPPDeque;
use crate::raw_ocpp_common_call::{RawOcppCommonCall, RawOcppCommonError};
use async_trait::async_trait;
use rust_ocpp::v1_6::messages::trigger_message::{TriggerMessageRequest, TriggerMessageResponse};
use rust_ocpp::v1_6::types::TriggerMessageStatus;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

pub struct OCPPCommunicator1_6 {
    pub client: OCPP1_6Client,
    pub data: Arc<Mutex<CPData>>,
    pub trigger_message_requests: Option<mpsc::Sender<String>>,
    pub ocpp_deque: OCPPDeque,
}

#[async_trait]
impl OCPPCommunicator for OCPPCommunicator1_6 {
    async fn send_boot_notification(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data: tokio::sync::MutexGuard<'_, CPData> = self.data.lock().await;
        let boot_notification: BootNotificationRequest = BootNotificationRequest {
            charge_point_model: data.model.clone(),
            charge_point_vendor: data.vendor.clone(),
            charge_point_serial_number: Some(data.serial.clone()),
            ..Default::default()
        };
        let message_id = Uuid::new_v4();

        let cp_data_arc = Arc::clone(&self.data);
        let callback = move |call: RawOcppCommonCall,
                             ocpp_result: Result<Value, RawOcppCommonError>,
                             _: Option<MessageReference>,
                             self_clone: OCPPDeque| {
            println!("BootNotification callback invoked");
            let cp_data_arc2 = Arc::clone(&cp_data_arc);
            async move {
                println!("BootNotification callback invoked1");

                println!(
                    "BootNotification response received for message ID {}: {:?}",
                    call.1, ocpp_result
                );
                let mut lock = cp_data_arc2.lock().await;
                lock.booted = true;
                println!("Booted set to true");
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
            println!("Boot is scheduled");
        }
        Ok(())
    }

    async fn send_authorize(
        &self,
        authorization_data: AuthorizationType,
        message_reference: MessageReference,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let AuthorizationType::RFID(rfid) = &authorization_data {
            println!("Sending authorize for RFID: {}", rfid.id_tag);
            let authorize_request = AuthorizeRequest {
                id_tag: rfid.id_tag.clone(),
            };
            //TODO Some Reference
            let response = self
                .ocpp_deque
                .do_send_request_queued(authorize_request, "Authorize", Some(message_reference))
                .await;

            //  let response = self.ocpp_deque.do_send_request_raw::<BootNotificationResponse>(message_id, call, "BootNotification").await;

            if let Ok(_) = response {
                println!("Authorize is schedule");
            } else {
                println!("Authorize failed to schedule");
            }

            let cp_data_arc = Arc::clone(&self.data);
            let callback = move |call: RawOcppCommonCall,
                                 ocpp_result: Result<Value, RawOcppCommonError>,
                                 reference: Option<MessageReference>,
                                 self_clone: OCPPDeque| {
                println!("Authorize callback invoked");
                let cp_data_arc2 = Arc::clone(&cp_data_arc);
                async move {
                    println!("Authorize callback invoked1");
                    let authorize_response: AuthorizeResponse =
                        serde_json::from_value(ocpp_result.unwrap()).unwrap(); //todo handle error
                    println!(
                        "Authorize response received for message ID {}: {:?}",
                        call.1, authorize_response
                    );

                    let mut lock = cp_data_arc2.lock().await;
                    if authorize_response.id_tag_info.status
                        == crate::rust_ocpp::v1_6::types::AuthorizationStatus::Accepted
                    {
                        if let Some(MessageReference::ChargeSession(cs_ref)) = reference {
                            println!(
                                "Authorization accepted for ID tag for evse {}, session {}",
                                cs_ref.evse_index, cs_ref.charge_session_index
                            );
                            lock.evses[cs_ref.evse_index].charge_sessions
                                [cs_ref.charge_session_index]
                                .authorized = true;
                        }
                    } else {
                        if let Some(MessageReference::ChargeSession(cs_ref)) = reference {
                            println!(
                                "Authorization re for ID tag for evse {}, session {}",
                                cs_ref.evse_index, cs_ref.charge_session_index
                            );
                            lock.evses[cs_ref.evse_index].charge_sessions
                                [cs_ref.charge_session_index]
                                .authorized = false;
                            //TODO, above not needed
                        }
                        println!("Authorization denied for ID tag");
                    }
                }
            };

            self.ocpp_deque
                .handle_on_response(callback, "Authorize")
                .await;
        } else {
            println!("Cannot send authorize for ocpp1_6 only RFID is supported");
        }

        Ok(())
    }

    async fn register_messages(&self) -> () {
        let _ = self.register_trigger_message().await;
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
                println!("Received TriggerMessage from server");
                if let Some(sender) = trigger_message_requests {
                    let _ = sender.send("boot_notification".to_string()).await;
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
}
