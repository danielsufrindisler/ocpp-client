use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[allow(dead_code)]
pub(crate) struct RawOcpp2_0_1Call(pub u64, pub String, pub String, pub Value);
