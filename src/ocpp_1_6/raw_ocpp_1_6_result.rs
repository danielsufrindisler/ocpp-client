use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct RawOcpp1_6Result(pub u64, pub String, pub Value);
