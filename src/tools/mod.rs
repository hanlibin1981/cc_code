//! 工具模块
//! 工具注册表和调用抽象

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 工具定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: ToolInputSchema,
}

/// 工具输入模式
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolInputSchema {
    #[serde(default)]
    pub properties: HashMap<String, SchemaProperty>,
    #[serde(default)]
    pub required: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaProperty {
    #[serde(rename = "type")]
    pub param_type: String,
    pub description: Option<String>,
}

/// 工具调用请求
#[derive(Debug, Clone)]
#[allow(unused)] pub struct ToolCall {
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

/// 工具执行结果
#[derive(Debug, Clone)]
#[allow(unused)] pub struct ToolExecutionResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

impl ToolExecutionResult {
    #[allow(unused)] pub fn ok(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            error: None,
        }
    }

    #[allow(unused)] pub fn err(error: impl Into<String>) -> Self {
        Self {
            success: false,
            output: String::new(),
            error: Some(error.into()),
        }
    }
}

/// 工具注册表
#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    tools: HashMap<String, ToolDef>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut registry = Self::default();
        registry.register_builtin_tools();
        registry
    }

    /// 注册内置工具
    fn register_builtin_tools(&mut self) {
        // cc_code 会话管理工具（通过 MCP 提供）
        // 注意：文件/Bash 等操作由 OpenClaw 执行，cc_code 只输出 [TOOL_CALL:...] 指令

        self.register(ToolDef {
            name: "cc_start_session".into(),
            description: "启动一个新的编程会话，返回 session_id".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "cwd".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("工作目录路径".into()),
                        },
                    );
                    props
                },
                required: vec!["cwd".to_string()],
            },
        });

        self.register(ToolDef {
            name: "cc_send_message".into(),
            description: "向 cc_code 发送消息进行推理。cc_code 返回的响应中包含 [TOOL_CALL:...] 格式的工具调用指令，OpenClaw 应解析并执行。".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "session_id".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("会话 ID".into()),
                        },
                    );
                    props.insert(
                        "message".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("任务描述或工具执行结果".into()),
                        },
                    );
                    props.insert(
                        "tool_results".to_string(),
                        SchemaProperty {
                            param_type: "array".into(),
                            description: Some(r#"可选，上一轮工具执行结果数组 [{tool: "name", result: "..."}]"#.into()),
                        },
                    );
                    props
                },
                required: vec!["session_id".to_string(), "message".to_string()],
            },
        });

        // 流式消息工具
        self.register(ToolDef {
            name: "cc_stream_message".into(),
            description: "流式版本的消息处理，使用流式 API 返回增量结果。响应中同样包含 [TOOL_CALL:...] 格式的工具调用。".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "session_id".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("会话 ID".into()),
                        },
                    );
                    props.insert(
                        "message".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("任务描述或工具执行结果".into()),
                        },
                    );
                    props.insert(
                        "tool_results".to_string(),
                        SchemaProperty {
                            param_type: "array".into(),
                            description: Some(r#"可选，上一轮工具执行结果数组 [{tool: "name", result: "..."}]"#.into()),
                        },
                    );
                    props
                },
                required: vec!["session_id".to_string(), "message".to_string()],
            },
        });

        self.register(ToolDef {
            name: "cc_list_sessions".into(),
            description: "列出所有活跃会话".into(),
            input_schema: ToolInputSchema::default(),
        });

        self.register(ToolDef {
            name: "cc_stop_session".into(),
            description: "停止指定会话".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "session_id".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("会话 ID".into()),
                        },
                    );
                    props
                },
                required: vec!["session_id".to_string()],
            },
        });
    }

    /// 注册工具
    pub fn register(&mut self, tool: ToolDef) {
        self.tools.insert(tool.name.clone(), tool);
    }

    #[allow(dead_code)]
    pub fn get(&self, name: &str) -> Option<&ToolDef> {
        self.tools.get(name)
    }

    /// 列出所有工具
    pub fn list(&self) -> Vec<&ToolDef> {
        self.tools.values().collect()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}
