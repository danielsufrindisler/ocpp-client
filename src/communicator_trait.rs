use crate::cp_data::{
    AuthorizationType, CPData, ChargeSession, MessageReference, PlugAndCharge, EV, EVSE, RFID,
};
use async_trait::async_trait;
use rust_ocpp::v1_6::messages::boot_notification::{
    self, BootNotificationRequest as BootNotificationRequest1_6,
    BootNotificationResponse as BootNotificationResponse1_6,
};
use rust_ocpp::v1_6::messages::trigger_message::{TriggerMessageRequest, TriggerMessageResponse};
use rust_ocpp::v1_6::types::TriggerMessageStatus;
use rust_ocpp::v2_0_1::messages::boot_notification::BootNotificationRequest as BootNotificationRequest2_0_1;
use rust_ocpp::v2_0_1::messages::status_notification::StatusNotificationRequest as StatusNotificationRequest2_0_1;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

#[async_trait]
pub trait OCPPCommunicator: Send + Sync {
    async fn send_boot_notification(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn send_authorize(
        &self,
        authorization_data: AuthorizationType,
        message_reference: MessageReference,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn register_trigger_message(
        &self,
    ) -> Result<TriggerMessageResponse, Box<dyn std::error::Error + Send + Sync>>;
    async fn register_messages(&self) -> ();
}
