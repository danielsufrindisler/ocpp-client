use crate::common_client::CommonOcppClientBase;
use crate::cp_data::{ChargeSessionReference, MessageReference};
use crate::raw_ocpp_common_call::RawOcppCommonCall;
use crate::raw_ocpp_common_call::{RawOcppCommonError, RawOcppCommonResult};
use crate::Client::{OCPP1_6, OCPP2_0_1};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::{
    mpsc,
    mpsc::{Receiver, Sender},
    Mutex,
};
use uuid::Uuid;
pub struct OcppQueuedMessage {
    uuid: Uuid,
    message: RawOcppCommonCall,
    reference: Option<MessageReference>,
}

#[derive(Clone)]
pub struct OCPPDeque {
    client: CommonOcppClientBase,
    callback_map: Arc<
        Mutex<
            BTreeMap<
                String,
                mpsc::Sender<(
                    RawOcppCommonCall,
                    Result<serde_json::Value, RawOcppCommonError>,
                    Option<MessageReference>,
                )>,
            >,
        >,
    >,
    sender: Sender<OcppQueuedMessage>,
}

impl OCPPDeque {
    pub fn new(client: CommonOcppClientBase) -> Self {
        let (sender, mut receiver) = mpsc::channel::<OcppQueuedMessage>(1000);
        let client2 = client.clone();

        let obj = Self {
            client: client,
            callback_map: Arc::new(Mutex::new(BTreeMap::new())),
            sender: sender,
        };
        let obj2 = obj.clone();
        tokio::spawn(async move {
            while let Some(call) = receiver.recv().await {
                let result = obj2
                    .send_with_retry(call.uuid, call.message.clone(), call.reference)
                    .await;
                println!("spawned do_send_request result: {:?}", result);
            }
        });
        obj
    }

    pub async fn send_with_retry(
        &self,
        message_id: Uuid,
        call: RawOcppCommonCall,
        message_ref: Option<MessageReference>,
    ) -> Result<Result<Value, RawOcppCommonError>, Box<dyn std::error::Error + Send + Sync>> {
        let mut retries = 3; //todo
        loop {
            let result = self
                .client
                .do_send_request_raw(message_id, call.clone())
                .await;

            if let (Ok(ref ocpp_result)) = result {
                if let mut callback_map = self.callback_map.lock().await {
                    if let Some(callback) = &mut callback_map.get(&call.2) {
                        println!("there is a callback for action {}", call.2);

                        callback
                            .send((
                                call,
                                match ocpp_result {
                                    Ok(value) => Ok(serde_json::to_value(value)?),
                                    Err(e) => Err(e.clone()),
                                },
                                message_ref,
                            ))
                            .await;
                    } else {
                        println!("there no callback for action{}", call.2);
                    }
                } else {
                    println!("there no callback lock{}", call.2);
                }
                break result;
            }

            retries -= 1;
            if (retries == 0) {
                break result;
            }
        }
    }

    pub async fn handle_on_response<
        F: FnMut(
                RawOcppCommonCall,
                Result<serde_json::Value, RawOcppCommonError>,
                Option<MessageReference>,
                Self,
            ) -> FF
            + Send
            + Sync
            + 'static,
        FF: Future<Output = ()> + Send + Sync,
        //F: FnMut(P, Self) -> FF + Send + Sync + 'static,
        //FF: Future<Output = Result<R, OCPP1_6Error>> + Send + Sync,
    >(
        &self,
        mut callback: F,
        action: &'static str,
    ) {
        let (sender, mut recv) = mpsc::channel(1000);
        {
            let mut lock = self.callback_map.lock().await;
            lock.insert(action.to_string(), sender);
        }

        let s = self.clone();
        println!("registered callback for action {}", action);

        tokio::spawn(async move {
            while let Some((call, result, reference)) = recv.recv().await {
                println!("calling callback");

                callback(call, result, reference, s.clone()).await;
            }
        });
    }

    pub async fn do_send_request_queued<P: Serialize>(
        &self,
        request: P,
        action: &str,
        message_reference: Option<MessageReference>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let message_id = Uuid::new_v4();

        let call: RawOcppCommonCall = RawOcppCommonCall(
            2,
            message_id.to_string(),
            action.to_string(),
            serde_json::to_value(&request)?,
        );
        self.sender
            .send(OcppQueuedMessage {
                uuid: message_id,
                message: call,
                reference: message_reference,
            })
            .await;

        Ok(())
    }

    pub async fn do_send_request_sync<P: Serialize, R: DeserializeOwned>(
        &self,
        request: P,
        action: &str,
    ) -> Result<Result<R, RawOcppCommonError>, Box<dyn std::error::Error + Send + Sync>> {
        let message_id = Uuid::new_v4();

        let call: RawOcppCommonCall = RawOcppCommonCall(
            2,
            message_id.to_string(),
            action.to_string(),
            serde_json::to_value(&request)?,
        );
        let result = self
            .client
            .do_send_request_raw(message_id, call.clone())
            .await;
        println!("do_send_request result: {:?}", result);

        if let Ok(ocpp_result) = &result {
            if let mut callback_map = self.callback_map.lock().await {
                if let Some(callback) = &mut callback_map.get(action) {
                    println!("there is a callback for action {}", action);

                    callback
                        .send((
                            call,
                            match ocpp_result {
                                Ok(value) => Ok(serde_json::to_value(value)?),
                                Err(e) => Err(e.clone()),
                            },
                            None,
                        ))
                        .await;
                }
            }
        }

        match result {
            Ok(ocpp_result) => match ocpp_result {
                Ok(value) => Ok(Ok(serde_json::from_value::<R>(value)?)),
                Err(e) => Ok(Err(e)),
            },
            Err(e) => Err(e),
        }
    }
}
