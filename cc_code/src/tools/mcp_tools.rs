//! MCP 工具调用
//! 生成 MCP 格式的工具调用请求

use crate::agent::ToolCallRequest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// MCP JSON-RPC 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpJsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

impl McpJsonRpcRequest {
    pub fn new(id: u64, method: &str) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params: None,
        }
    }

    pub fn with_params(mut self, params: serde_json::Value) -> Self {
        self.params = Some(params);
        self
    }
}

/// MCP 工具调用请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCallRequest {
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

/// 转换为 MCP 工具调用 JSON-RPC 请求
pub fn to_mcp_tool_call(tool_call: &ToolCallRequest, id: u64) -> McpJsonRpcRequest {
    let params = serde_json::json!({
        "name": tool_call.name,
        "arguments": tool_call.arguments,
    });

    McpJsonRpcRequest::new(id, "tools/call").with_params(params)
}

/// 解析 MCP 工具调用响应
#[derive(Debug, Clone, Deserialize)]
pub struct McpToolCallResponse {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    #[serde(default)]
    pub result: Option<McpToolResult>,
    #[serde(default)]
    pub error: Option<McpError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpToolResult {
    pub content: Vec<McpContentBlock>,
    #[serde(default)]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum McpContentBlock {
    Text {
        text: String,
    },
    Image {
        data: String,
        mime_type: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
}

impl McpToolCallResponse {
    /// 从响应中提取文本内容
    pub fn extract_text(&self) -> String {
        if let Some(ref result) = self.result {
            let texts: Vec<String> = result
                .content
                .iter()
                .filter_map(|block| {
                    if let McpContentBlock::Text { text } = block {
                        Some(text.clone())
                    } else {
                        None
                    }
                })
                .collect();
            texts.join("\n")
        } else if let Some(ref error) = self.error {
            format!("错误 ({}): {}", error.code, error.message)
        } else {
            "未知响应".into()
        }
    }

    /// 检查是否有错误
    pub fn is_error(&self) -> bool {
        self.error.is_some()
            || self
                .result
                .as_ref()
                .map(|r| r.is_error.unwrap_or(false))
                .unwrap_or(false)
    }
}

/// MCP 服务器信息
#[derive(Debug, Clone, Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    pub version: String,
}

/// 工具列表响应
#[derive(Debug, Clone, Deserialize)]
pub struct McpToolsListResponse {
    pub tools: Vec<McpToolDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub input_schema: serde_json::Value,
}
