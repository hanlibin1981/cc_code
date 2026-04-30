//! MCP 协议处理模块
//! 实现 JSON-RPC 2.0 协议 + MCP 扩展

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// JSON-RPC 2.0 请求
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(unused)] pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 响应
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    #[allow(unused)] pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// MCP 协议方法名
pub mod method {
    // 初始化
    #[allow(unused)] pub const INITIALIZE: &str = "initialize";
    #[allow(unused)] pub const INITIALIZED: &str = "initialized";

    // 工具
    #[allow(unused)] pub const TOOLS_LIST: &str = "tools/list";
    #[allow(unused)] pub const TOOLS_CALL: &str = "tools/call";

    // 资源
    #[allow(unused)] pub const RESOURCES_LIST: &str = "resources/list";
    #[allow(unused)] pub const RESOURCES_READ: &str = "resources/read";

    // prompts
    #[allow(unused)] pub const PROMPTS_LIST: &str = "prompts/list";

    // 通知
    #[allow(unused)] pub const NOTIFICATION_INITIALIZED: &str = "notifications/initialized";
}

/// MCP 服务器能力
#[derive(Debug, Clone, Default, Serialize)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolsCapability {
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourcesCapability {
    pub subscribe: bool,
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptsCapability {
    pub list_changed: bool,
}

/// 工具定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
}

impl Tool {
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema: Value::Object(serde_json::Map::new()),
        }
    }
}

/// 工具调用结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    Text { text: String },
    Image { data: String, mime_type: String },
    Resource { resource: ResourceContent },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContent {
    pub uri: String,
    pub mime_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub blob: Option<String>,
}

/// 工具调用输入
#[derive(Debug, Clone, Deserialize)]
pub struct CallToolInput {
    pub name: String,
    #[serde(default)]
    pub arguments: HashMap<String, Value>,
}

/// 列表工具响应
#[derive(Debug, Clone, Serialize)]
pub struct ListToolsResult {
    pub tools: Vec<Tool>,
}

/// 协议版本
#[allow(unused)]
#[derive(Debug, Clone, Deserialize)]
pub struct ProtocolVersion {
    pub major: u32,
    pub minor: u32,
}

/// 初始化请求参数
#[allow(unused)]
#[derive(Debug, Clone, Deserialize)]
pub struct InitializeParams {
    pub protocol_version: ProtocolVersion,
    pub capabilities: ClientCapabilities,
    pub client_info: ClientInfo,
}

/// 客户端能力
#[allow(unused)]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClientCapabilities {
    #[serde(default)]
    pub tools: Option<Value>,
    #[serde(default)]
    pub resources: Option<Value>,
}

/// 客户端信息
#[allow(unused)]
#[derive(Debug, Clone, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}
