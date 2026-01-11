use ocpp_client::Client::{OCPP1_6, OCPP2_0_1};
use ocpp_client::{connect, ConnectOptions};
use rust_ocpp::v1_6::messages::boot_notification::{self, BootNotificationRequest as BootNotificationRequest1_6};
use rust_ocpp::v2_0_1::messages::boot_notification::BootNotificationRequest as BootNotificationRequest2_0_1;

use async_trait::async_trait;
use ocpp_client::ocpp_1_6::OCPP1_6Client;
use ocpp_client::ocpp_2_0_1::OCPP2_0_1Client;
use rust_ocpp::v1_6::messages::trigger_message::{TriggerMessageRequest, TriggerMessageResponse};
use rust_ocpp::v1_6::types::TriggerMessageStatus;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use ocpp_client::ocpp_1_6::raw_ocpp_1_6_call;
use uuid::Uuid;
#[async_trait]
trait OCPPCommunicator: Send + Sync {
    async fn send_boot_notification(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn register_trigger_message(
        &self,
    ) -> Result<TriggerMessageResponse, Box<dyn std::error::Error + Send + Sync>>;
    async fn register_messages(&self) -> ();
}

struct OCPPCommunicator1_6 {
    client: OCPP1_6Client,
    data: Arc<CPData>,
    tx: Option<mpsc::Sender<String>>,
}

struct OCPPCommunicator2_0_1 {
    client: OCPP2_0_1Client,
    data: Arc<CPData>,
    tx: Option<mpsc::Sender<String>>,
}

#[async_trait]
impl OCPPCommunicator for OCPPCommunicator1_6 {
    async fn send_boot_notification(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
       

       let boot_notification: BootNotificationRequest1_6 = BootNotificationRequest1_6 {
                charge_point_model: self.data.model.clone(),
                charge_point_vendor: self.data.vendor.clone(),
                charge_point_serial_number: Some(self.data.serial.clone()),
                ..Default::default()
            };
         let message_id = Uuid::new_v4();

        let call: RawOcpp1_6Call = RawOcpp1_6Call(
            2,
            message_id.to_string(),
            action.to_string(),
            serde_json::to_value(&boot_notification)?,
        );
        let response = self.do_send_request_raw(message_id, call, action).await;

        match response {
             Ok(value) => {
                println!("{}", value.current_time); 
                
             }
            _ => Ok(())
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
        let tx = self.tx.clone();
        let callback = move |_request: TriggerMessageRequest, _client: OCPP1_6Client| {
            let _data = data.clone();
            let tx = tx.clone();
            async move {
                println!("Received TriggerMessage from server");
                if let Some(sender) = tx {
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

#[async_trait]
impl OCPPCommunicator for OCPPCommunicator2_0_1 {
    async fn send_boot_notification(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _ = self
            .client
            .send_boot_notification(BootNotificationRequest2_0_1 {
                charging_station:
                    rust_ocpp::v2_0_1::datatypes::charging_station_type::ChargingStationType {
                        model: self.data.model.clone(),
                        vendor_name: self.data.vendor.clone(),
                        serial_number: Some(self.data.serial.clone()),
                        ..Default::default()
                    },
                ..Default::default()
            })
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
}

struct CPData {
    username: Option<String>,
    password: Option<String>,
    serial: String,
    model: String,
    vendor: String,
}

struct CP {
    communicator: Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>>,
    data: Arc<CPData>,
    client: Option<ocpp_client::Client>,
}

impl CP {
    pub fn communicator(&self) -> Arc<Mutex<Option<Box<dyn OCPPCommunicator>>>> {
        self.communicator.clone()
    }

    pub async fn run(
        &mut self,
        address: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let options: ConnectOptions = ConnectOptions {
            username: self.data.username.as_deref(),
            password: self.data.password.as_deref(),
        };
        let client = connect(address, Some(options)).await?;
        // Create mpsc channel
        let (tx, mut rx) = mpsc::channel::<String>(100);

        match &client {
            OCPP1_6(inner) => {
                *self.communicator.lock().await = Some(Box::new(OCPPCommunicator1_6 {
                    client: inner.clone(),
                    data: self.data.clone(),
                    tx: Some(tx),
                })
                    as Box<dyn OCPPCommunicator>);
                println!("Connected using OCPP 1.6");
            }
            OCPP2_0_1(inner) => {
                *self.communicator.lock().await = Some(Box::new(OCPPCommunicator2_0_1 {
                    client: inner.clone(),
                    data: self.data.clone(),
                    tx: Some(tx),
                })
                    as Box<dyn OCPPCommunicator>);
                println!("Connected using OCPP 2.0.1");
            }
        };

        self.client = Some(client);

        // Spawn task to listen for messages on the mpsc channel
        let communicator = self.communicator.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if msg == "boot_notification" {
                    println!("Received boot_notification message from mpsc");
                    if let Some(comm) = communicator.lock().await.as_ref() {
                        let _ = comm.send_boot_notification().await;
                    }
                }
            }
        });

        if let Some(comm) = self.communicator.lock().await.as_ref() {
            comm.register_messages().await;
            comm.send_boot_notification().await?
        }

        println!("Connected! Waiting for messages...");
        println!("Press Ctrl-C to exit");
        tokio::signal::ctrl_c().await?;

        println!("Disconnecting...");
        if let Some(ocpp_client::Client::OCPP1_6(client)) = &self.client {
            let _ = client.disconnect().await;
        } else if let Some(ocpp_client::Client::OCPP2_0_1(client)) = &self.client {
            let _ = client.disconnect().await;
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "debug");
    }

    env_logger::init();

    let mut cp1 = CP {
        communicator: Arc::new(Mutex::new(None)),
        client: None,
        data: Arc::new(CPData {
            username: Some("RustTest002".to_string()),
            password: Some("RustyRust".to_string()),
            serial: "RustTest002".to_string(),
            model: "RustModel".to_string(),
            vendor: "RustVendor".to_string(),
        }),
    };

    cp1.run("wss://ocpp.coreevi.com/ocpp1.6/65/RustTest002")
        .await?;

    Ok(())
}
