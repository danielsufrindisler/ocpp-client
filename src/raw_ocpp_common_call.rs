use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawOcppCommonCall(pub u64, pub String, pub String, pub Value);

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawOcppCommonError(pub u64, pub String, pub String, pub String, pub Value);

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawOcppCommonResult(pub u64, pub String, pub Value);
