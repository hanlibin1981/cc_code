//! 工具模块
//! 工具注册表和调用抽象

mod mcp_tools;

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
pub struct ToolCall {
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

/// 工具执行结果
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
        // cc_code 会话管理工具
        self.register(ToolDef {
            name: "cc_start_session".into(),
            description: "启动一个新的编程会话".into(),
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
            description: "向 cc_code 发送消息".into(),
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
                            description: Some("任务描述".into()),
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

        // 本地文件/Bash 工具
        self.register(ToolDef {
            name: "read_file".into(),
            description: "读取文件内容".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "path".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("文件路径".into()),
                        },
                    );
                    props
                },
                required: vec!["path".to_string()],
            },
        });

        self.register(ToolDef {
            name: "write_file".into(),
            description: "写入文件内容".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "path".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("文件路径".into()),
                        },
                    );
                    props.insert(
                        "content".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("文件内容".into()),
                        },
                    );
                    props
                },
                required: vec!["path".to_string(), "content".to_string()],
            },
        });

        self.register(ToolDef {
            name: "edit_file".into(),
            description: "编辑文件（替换指定文本）".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "path".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("文件路径".into()),
                        },
                    );
                    props.insert(
                        "old_text".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("要替换的旧文本".into()),
                        },
                    );
                    props.insert(
                        "new_text".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("替换后的新文本".into()),
                        },
                    );
                    props
                },
                required: vec!["path".to_string(), "old_text".to_string()],
            },
        });

        self.register(ToolDef {
            name: "bash".into(),
            description: "执行 Shell 命令".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "command".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("Shell 命令".into()),
                        },
                    );
                    props
                },
                required: vec!["command".to_string()],
            },
        });

        self.register(ToolDef {
            name: "glob".into(),
            description: "搜索匹配模式的所有文件".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "pattern".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("文件名匹配模式 (e.g. *.rs)".into()),
                        },
                    );
                    props.insert(
                        "base_dir".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("搜索基础目录 (default: .)".into()),
                        },
                    );
                    props
                },
                required: vec!["pattern".to_string()],
            },
        });

        self.register(ToolDef {
            name: "grep".into(),
            description: "在文件中搜索文本".into(),
            input_schema: ToolInputSchema {
                properties: {
                    let mut props = HashMap::new();
                    props.insert(
                        "pattern".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("搜索模式".into()),
                        },
                    );
                    props.insert(
                        "path".to_string(),
                        SchemaProperty {
                            param_type: "string".into(),
                            description: Some("搜索路径 (default: .)".into()),
                        },
                    );
                    props
                },
                required: vec!["pattern".to_string()],
            },
        });
    }

    /// 注册工具
    pub fn register(&mut self, tool: ToolDef) {
        self.tools.insert(tool.name.clone(), tool);
    }

    /// 获取工具
    pub fn get(&self, name: &str) -> Option<&ToolDef> {
        self.tools.get(name)
    }

    /// 列出所有工具
    pub fn list(&self) -> Vec<&ToolDef> {
        self.tools.values().collect()
    }

    /// 工具数量
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}
