//! 工具模块

pub mod executor;
pub mod file_tool;
pub mod bash_tool;
pub mod edit_tool;
pub mod search_tool;

pub use executor::ToolExecutor;
pub use file_tool::FileTool;
pub use bash_tool::BashTool;
pub use edit_tool::EditTool;
pub use search_tool::SearchTool;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: ToolInputSchema,
}

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

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

impl ToolExecutionResult {
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            error: None,
        }
    }

    pub fn err(error: impl Into<String>) -> Self {
        Self {
            success: false,
            output: String::new(),
            error: Some(error.into()),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        if self.success {
            serde_json::json!({"success": true, "output": self.output})
        } else {
            serde_json::json!({"success": false, "error": self.error})
        }
    }
}

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

    fn register_builtin_tools(&mut self) {
        self.register(ToolDef {
            name: "cc_start_session".into(),
            description: "启动一个新的编程会话".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert("cwd".to_string(), SchemaProperty {
                        param_type: "string".into(),
                        description: Some("工作目录路径".into()),
                    });
                    props
                },
                required: vec!["cwd".to_string()],
            },
        });

        self.register(ToolDef {
            name: "cc_send_message".into(),
            description: "向 cc_code 发送消息进行推理".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert("session_id".to_string(), SchemaProperty {
                        param_type: "string".into(),
                        description: Some("会话 ID".into()),
                    });
                    props.insert("message".to_string(), SchemaProperty {
                        param_type: "string".into(),
                        description: Some("任务描述".into()),
                    });
                    props
                },
                required: vec!["session_id".to_string(), "message".to_string()],
            },
        });

        self.register(ToolDef {
            name: "cc_stream_message".into(),
            description: "流式版本的消息处理".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert("session_id".to_string(), SchemaProperty {
                        param_type: "string".into(),
                        description: Some("会话 ID".into()),
                    });
                    props.insert("message".to_string(), SchemaProperty {
                        param_type: "string".into(),
                        description: Some("任务描述".into()),
                    });
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
                    props.insert("session_id".to_string(), SchemaProperty {
                        param_type: "string".into(),
                        description: Some("会话 ID".into()),
                    });
                    props
                },
                required: vec!["session_id".to_string()],
            },
        });


        // 文件搜索工具
        self.register(ToolDef {
            name: "glob".into(),
            description: "按 glob 模式搜索文件（如 **/*.rs, *.txt）".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert("pattern".to_string(), SchemaProperty {
                        param_type: "string".into(),
                        description: Some("glob 模式（如 **/*.rs）".into()),
                    });
                    props.insert("cwd".to_string(), SchemaProperty {
                        param_type: "string".into(),
                        description: Some("搜索根目录（可选，默认为当前目录）".into()),
                    });
                    props
                },
                required: vec!["pattern".to_string()],
            },
        });

        self.register(ToolDef {
            name: "grep".into(),
            description: "在文件中搜索内容（支持正则表达式）".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert("pattern".to_string(), SchemaProperty {
                        param_type: "string".into(),
                        description: Some("搜索模式（正则表达式）".into()),
                    });
                    props.insert("paths".to_string(), SchemaProperty {
                        param_type: "array".into(),
                        description: Some("搜索的文件路径数组（可选，空表示搜索所有文件）".into()),
                    });
                    props.insert("cwd".to_string(), SchemaProperty {
                        param_type: "string".into(),
                        description: Some("搜索根目录（可选）".into()),
                    });
                    props.insert("case_sensitive".to_string(), SchemaProperty {
                        param_type: "boolean".into(),
                        description: Some("是否大小写敏感（默认 false）".into()),
                    });
                    props
                },
                required: vec!["pattern".to_string()],
            },
        });
    }

    pub fn register(&mut self, tool: ToolDef) {
        self.tools.insert(tool.name.clone(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&ToolDef> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&ToolDef> {
        self.tools.values().collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}
