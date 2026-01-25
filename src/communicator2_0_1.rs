use crate::Client::{OCPP1_6, OCPP2_0_1};
use crate::{connect, ConnectOptions};
use rust_ocpp::v2_0_1::messages::authorize::{AuthorizeRequest, AuthorizeResponse};
use rust_ocpp::v2_0_1::messages::boot_notification::BootNotificationRequest as BootNotificationRequest2_0_1;

use crate::communicator_trait::OCPPCommunicator;
use crate::cp_data::{
    AuthorizationType, CPData, ChargeSession, MessageReference, PlugAndCharge, EV, EVSE, RFID,
};
use crate::ocpp_2_0_1::OCPP2_0_1Client;
use crate::ocpp_deque::OCPPDeque;
use crate::raw_ocpp_common_call::{RawOcppCommonCall, RawOcppCommonError};
use async_trait::async_trait;
use rust_ocpp::v1_6::messages::trigger_message::{TriggerMessageRequest, TriggerMessageResponse};
use rust_ocpp::v1_6::types::TriggerMessageStatus;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

pub struct OCPPCommunicator2_0_1 {
    pub client: OCPP2_0_1Client,
    pub data: Arc<Mutex<CPData>>,
    pub trigger_message_requests: Option<mpsc::Sender<String>>,
    pub ocpp_deque: OCPPDeque,
}

#[async_trait]
impl OCPPCommunicator for OCPPCommunicator2_0_1 {
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
}
