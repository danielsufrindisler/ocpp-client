use crate::common_client::CommonOcppClientBase;
use crate::cp_data::{AuthorizationType, ChargeSessionReference, MessageReference};
use async_trait::async_trait;
use rust_ocpp::v1_6::messages::trigger_message::TriggerMessageResponse;

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
    async fn send_start_transaction(
        &self,
        reference: ChargeSessionReference,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn send_stop_transaction(
        &self,
        reference: ChargeSessionReference,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn send_meter_values(
        &self,
        reference: ChargeSessionReference,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    fn get_base(&mut self) -> &mut CommonOcppClientBase;
}
