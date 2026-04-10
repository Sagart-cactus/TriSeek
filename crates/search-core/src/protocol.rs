use crate::query::QueryRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DAEMON_HOST: &str = "127.0.0.1";
pub const DAEMON_PID_FILE: &str = "daemon.pid";
pub const DAEMON_PORT_FILE: &str = "daemon.port";

/// JSON-RPC 2.0 request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: Value,
}

/// JSON-RPC 2.0 response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

impl RpcResponse {
    pub fn ok(id: u64, result: impl Serialize) -> Self {
        RpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(serde_json::to_value(result).unwrap_or(Value::Null)),
            error: None,
        }
    }

    pub fn error(id: u64, code: i32, message: impl Into<String>) -> Self {
        RpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

/// Response for the `status` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub daemon_dir: String,
    pub uptime_secs: u64,
    pub active_roots: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<DaemonRootStatus>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonRootStatus {
    pub target_root: String,
    pub index_dir: String,
    pub index_available: bool,
    pub generation: u64,
    pub delta_docs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonSearchParams {
    pub target_root: String,
    pub request: QueryRequest,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonStatusParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_root: Option<String>,
}

/// Parameters for the `frecency_select` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct FrecencySelectParams {
    pub target_root: String,
    pub paths: Vec<String>,
}
