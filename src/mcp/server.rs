//! MCP 服务器模块

use crate::mcp::protocol::{
    methods, success_response, error_response, JsonRpcRequest, JsonRpcResponse, JsonRpcError,
    ToolCallResult, ServerCapabilities,
};
use crate::tools::{ToolExecutor, ToolRegistry, ToolCall};
use crate::session::SessionManager;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

pub struct McpServer {
    tool_executor: Arc<ToolExecutor>,
    tool_registry: Arc<RwLock<ToolRegistry>>,
    session_manager: Arc<RwLock<SessionManager>>,
}

impl McpServer {
    pub fn new(tool_executor: ToolExecutor, session_manager: SessionManager) -> Self {
        Self {
            tool_executor: Arc::new(tool_executor),
            tool_registry: Arc::new(RwLock::new(ToolRegistry::new())),
            session_manager: Arc::new(RwLock::new(session_manager)),
        }
    }

    pub async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let id = request.id.clone();

        match request.method.as_str() {
            methods::PING => {
                success_response(id, serde_json::json!({"pong": true}))
            }
            methods::INITIALIZE => {
                let capabilities = ServerCapabilities::new();
                let result = serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": capabilities.capabilities,
                    "serverInfo": {
                        "name": "cc_code",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                });
                info!("Client initialized with protocol version: 2024-11-05");
                success_response(id, result)
            }
            methods::INITIALIZED => {
                info!("Client initialization complete");
                success_response(id, serde_json::json!({}))
            }
            methods::TOOLS_LIST => {
                let registry = self.tool_registry.read().await;
                let tools: Vec<_> = registry.list().into_iter().map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "inputSchema": t.input_schema
                    })
                }).collect();
                success_response(id, serde_json::json!({"tools": tools}))
            }
            methods::TOOLS_CALL => {
                let result = self.handle_tool_call(request.params).await;
                success_response(id, result)
            }
            methods::RESOURCES_LIST => {
                let resources = serde_json::json!({
                    "resources": [
                        {"uri": "workspace://current", "name": "Current Workspace", "mimeType": "application/json"}
                    ]
                });
                success_response(id, resources)
            }
            #[allow(unreachable_patterns)]
            _ => {
                warn!("Unknown method: {}", request.method);
                error_response(id, JsonRpcError::method_not_found())
            }
        }
    }

    async fn handle_tool_call(&self, params: serde_json::Value) -> serde_json::Value {
        let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let arguments = params.get("arguments")
            .and_then(|a| a.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default();

        let tool_call = ToolCall {
            name: name.to_string(),
            arguments,
        };

        let result = self.tool_executor.execute(&tool_call).await;
        let call_result = ToolCallResult::text(&result.output);
        
        if result.success {
            serde_json::to_value(&call_result).unwrap_or_else(|_| serde_json::json!({"content": []}))
        } else {
            let error_result = ToolCallResult::error(&result.error.unwrap_or_default());
            serde_json::to_value(&error_result).unwrap_or_else(|_| serde_json::json!({"content": []}))
        }
    }

    pub async fn list_tools(&self) -> Vec<crate::tools::ToolDef> {
        let registry = self.tool_registry.read().await;
        registry.list().into_iter().cloned().collect()
    }

    pub async fn register_tool(&self, tool: crate::tools::ToolDef) {
        let mut registry = self.tool_registry.write().await;
        registry.register(tool);
    }
}

pub async fn process_message(server: &McpServer, message: &str) -> Option<String> {
    let request = match crate::mcp::protocol::parse_request(message) {
        Ok(r) => r,
        Err(e) => {
            let response = error_response(serde_json::Value::Null, e);
            return serde_json::to_string(&response).ok();
        }
    };

    let response = server.handle_request(request).await;
    serde_json::to_string(&response).ok()
}

