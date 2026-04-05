use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use log::{trace};

/// Statistics for a specific message type
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct MessageTypeStats {
    pub message_type: String,
    pub sent_count: u64,
    pub sent_success: u64,
    pub sent_fail: u64,
    pub received_count: u64,
    pub received_success: u64,
    pub received_fail: u64,
}

impl MessageTypeStats {
    pub fn new(message_type: String) -> Self {
        Self {
            message_type,
            sent_count: 0,
            sent_success: 0,
            sent_fail: 0,
            received_count: 0,
            received_success: 0,
            received_fail: 0,
        }
    }

    pub fn record_sent(&mut self, success: bool) {
        trace!("Recording sent message of type {:?}: success={}", self.message_type, success);
        self.sent_count += 1;
        if success {
            self.sent_success += 1;
        } else {
            self.sent_fail += 1;
        }
    }

    pub fn record_received(&mut self, success: bool) {
        trace!("Recording received message of type {:?}: success={}", self.message_type, success);
        self.received_count += 1;
        if success {
            self.received_success += 1;
        } else {
            self.received_fail += 1;
        }
    }
}

/// Summary statistics for a single CP
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CPSummary {
    pub cp_id: usize,
    pub cp_serial: String,
    pub messages: HashMap<String, MessageTypeStats>,
    pub total_energy_delivered_kwh: f64,
}

impl CPSummary {
    pub fn new(cp_id: usize, cp_serial: String) -> Self {
        Self {
            cp_id,
            cp_serial,
            messages: HashMap::new(),
            total_energy_delivered_kwh: 0.0,
        }
    }

    pub fn record_sent(&mut self, message_type: &str, success: bool) {
        self.messages
            .entry(message_type.to_string())
            .or_insert_with(|| MessageTypeStats::new(message_type.to_string()))
            .record_sent(success);
    }

    pub fn record_received(&mut self, message_type: &str, success: bool) {
        self.messages
            .entry(message_type.to_string())
            .or_insert_with(|| MessageTypeStats::new(message_type.to_string()))
            .record_received(success);
    }

    pub fn add_energy(&mut self, kwh: f64) {
        self.total_energy_delivered_kwh += kwh;
    }
}

/// Overall test summary
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct TestSummary {
    pub cps: HashMap<usize, CPSummary>,
    pub start_time: String,
    pub end_time: String,
}

impl TestSummary {
    pub fn new() -> Self {
        Self {
            cps: HashMap::new(),
            start_time: chrono::Utc::now().to_rfc3339(),
            end_time: String::new(),
        }
    }

    pub fn finalize(&mut self) {
        self.end_time = chrono::Utc::now().to_rfc3339();
    }

    pub fn get_or_create_cp(&mut self, cp_id: usize, cp_serial: String) -> &mut CPSummary {
        self.cps
            .entry(cp_id)
            .or_insert_with(|| CPSummary::new(cp_id, cp_serial))
    }
}

/// Thread-safe message tracking
#[derive(Clone)]
pub struct MessageTracker {
    summary: Arc<Mutex<TestSummary>>,
}

impl MessageTracker {
    pub fn new() -> Self {
        Self {
            summary: Arc::new(Mutex::new(TestSummary::new())),
        }
    }

    pub async fn record_sent(&self, cp_id: usize, cp_serial: String, message_type: &str, success: bool) {
        let mut summary = self.summary.lock().await;
        let cp = summary.get_or_create_cp(cp_id, cp_serial);
        cp.record_sent(message_type, success);
    }

    pub async fn record_received(&self, cp_id: usize, cp_serial: String, message_type: &str, success: bool) {
        let mut summary = self.summary.lock().await;
        let cp = summary.get_or_create_cp(cp_id, cp_serial);
        cp.record_received(message_type, success);
    }

    pub async fn record_energy(&self, cp_id: usize, cp_serial: String, kwh: f64) {
        let mut summary = self.summary.lock().await;
        let cp = summary.get_or_create_cp(cp_id, cp_serial);
        cp.add_energy(kwh);
    }

    pub async fn get_summary(&self) -> TestSummary {
        let mut summary = self.summary.lock().await;
        summary.finalize();
        summary.clone()
    }
}
