//! MCP 协议模块

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn new(code: i32, message: &str) -> Self {
        Self {
            code,
            message: message.to_string(),
            data: None,
        }
    }

    pub fn parse_error() -> Self {
        Self::new(-32700, "Parse error")
    }

    pub fn invalid_request() -> Self {
        Self::new(-32600, "Invalid Request")
    }

    pub fn method_not_found() -> Self {
        Self::new(-32601, "Method not found")
    }

    pub fn invalid_params() -> Self {
        Self::new(-32602, "Invalid params")
    }

    pub fn internal_error(msg: &str) -> Self {
        Self::new(-32603, &format!("Internal error: {}", msg))
    }
}

pub mod protocol {
    pub const VERSION: &str = "2024-11-05";
    pub mod capabilities {
        pub const SAMPLING: &str = "sampling";
        pub const ROOTS: &str = "roots";
        pub const TOOLS: &str = "tools";
    }
}

pub mod methods {
    pub const INITIALIZE: &str = "initialize";
    pub const INITIALIZED: &str = "initialized";
    pub const TOOLS_LIST: &str = "tools/list";
    pub const TOOLS_CALL: &str = "tools/call";
    pub const RESOURCES_LIST: &str = "resources/list";
    pub const PING: &str = "ping";
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallResult {
    pub content: Vec<ToolCallContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: Option<String>,
}

impl ToolCallResult {
    pub fn text(text: &str) -> Self {
        Self {
            content: vec![ToolCallContent {
                content_type: "text".to_string(),
                text: Some(text.to_string()),
            }],
            is_error: None,
        }
    }

    pub fn error(text: &str) -> Self {
        Self {
            content: vec![ToolCallContent {
                content_type: "text".to_string(),
                text: Some(text.to_string()),
            }],
            is_error: Some(true),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ServerCapabilities {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub capabilities: Value,
}

impl ServerCapabilities {
    pub fn new() -> Self {
        Self {
            protocol_version: protocol::VERSION.to_string(),
            capabilities: serde_json::json!({
                "tools": {},
                "resources": {},
                "logging": {}
            }),
        }
    }
}

impl Default for ServerCapabilities {
    fn default() -> Self {
        Self::new()
    }
}

pub fn success_response(id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(result),
        error: None,
    }
}

pub fn error_response(id: Value, error: JsonRpcError) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: None,
        error: Some(error),
    }
}

pub fn parse_request(line: &str) -> Result<JsonRpcRequest, JsonRpcError> {
    serde_json::from_str(line).map_err(|_| JsonRpcError::parse_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_request() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let request: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(request.method, "initialize");
    }

    #[test]
    fn test_success_response() {
        let response = success_response(serde_json::json!(1), serde_json::json!({"result": "ok"}));
        assert!(response.error.is_none());
        assert!(response.result.is_some());
    }
}
