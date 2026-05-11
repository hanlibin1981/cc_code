//! 工具执行器模块

use crate::tools::{ToolCall, ToolExecutionResult};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct ToolExecutor {
    workspace: PathBuf,
    pub file_tool: crate::tools::FileTool,
    pub bash_tool: crate::tools::BashTool,
    pub edit_tool: crate::tools::EditTool,
    pub search_tool: crate::tools::SearchTool,
}

impl ToolExecutor {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            file_tool: crate::tools::FileTool::new(),
            bash_tool: crate::tools::BashTool::new(),
            edit_tool: crate::tools::EditTool::new(),
            search_tool: crate::tools::SearchTool::new(),
        }
    }

    pub async fn execute(&self, tool_call: &ToolCall) -> ToolExecutionResult {
        match tool_call.name.as_str() {
            "read_file" => {
                let path = self.get_string_arg(&tool_call.arguments, "path");
                match path {
                    Some(p) => self.file_tool.read_file(&p).await,
                    None => ToolExecutionResult::err("Missing 'path' argument"),
                }
            }
            "write_file" => {
                let path = self.get_string_arg(&tool_call.arguments, "path");
                let content = self.get_string_arg(&tool_call.arguments, "content");
                match (path, content) {
                    (Some(p), Some(c)) => self.file_tool.write_file(&p, &c).await,
                    _ => ToolExecutionResult::err("Missing arguments"),
                }
            }
            "bash" | "exec" => {
                let command = self.get_string_arg(&tool_call.arguments, "command");
                let timeout = self.get_u64_arg(&tool_call.arguments, "timeout_ms").unwrap_or(30000);
                match command {
                    Some(c) => self.bash_tool.execute(&c, timeout).await,
                    None => ToolExecutionResult::err("Missing 'command' argument"),
                }
            }
            "edit_file" => {
                let path = self.get_string_arg(&tool_call.arguments, "path");
                let old_text = self.get_string_arg(&tool_call.arguments, "oldText");
                let new_text = self.get_string_arg(&tool_call.arguments, "newText");
                match (path, old_text, new_text) {
                    (Some(p), Some(old), Some(new)) => self.edit_tool.edit_file(&p, &old, &new).await,
                    _ => ToolExecutionResult::err("Missing arguments for edit_file"),
                }
            }
            "list_directory" | "ls" => {
                let path = self.get_string_arg(&tool_call.arguments, "path");
                let p = path.unwrap_or_else(|| ".".to_string());
                self.file_tool.list_directory(&p).await
            }
            "glob" => {
                let pattern = self.get_string_arg(&tool_call.arguments, "pattern");
                let cwd = self.get_string_arg(&tool_call.arguments, "cwd");
                match pattern {
                    Some(p) => self.search_tool.glob(&p, cwd.as_deref()).await,
                    None => ToolExecutionResult::err("Missing 'pattern' argument"),
                }
            }
            "grep" => {
                let pattern = self.get_string_arg(&tool_call.arguments, "pattern");
                let paths = tool_call.arguments.get("paths")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>());
                let cwd = self.get_string_arg(&tool_call.arguments, "cwd");
                let case_sensitive = tool_call.arguments.get("case_sensitive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                match pattern {
                    Some(p) => self.search_tool.grep(&p, paths.as_deref(), cwd.as_deref(), case_sensitive).await,
                    None => ToolExecutionResult::err("Missing 'pattern' argument"),
                }
            }
            _ => ToolExecutionResult::err(format!("Unknown tool: {}", tool_call.name)),
        }
    }

    pub fn parse_tool_call(text: &str) -> Option<ToolCall> {
        let start = text.find("[TOOL_CALL:")?;
        let json_start = start + "[TOOL_CALL:".len();
        let json_end = text[json_start..].find(']')? + json_start;
        if json_end <= json_start {
            return None;
        }
        let json_str = &text[json_start..json_end].trim();
        let parsed: ToolCallJson = serde_json::from_str(json_str).ok()?;
        Some(ToolCall {
            name: parsed.name,
            arguments: parsed.arguments,
        })
    }

    fn get_string_arg(&self, args: &HashMap<String, serde_json::Value>, key: &str) -> Option<String> {
        args.get(key)?.as_str().map(|s| s.to_string())
    }

    fn get_u64_arg(&self, args: &HashMap<String, serde_json::Value>, key: &str) -> Option<u64> {
        args.get(key)?.as_u64()
    }
}

#[derive(Debug, serde::Deserialize)]
struct ToolCallJson {
    name: String,
    arguments: HashMap<String, serde_json::Value>,
}

pub fn parse_tool_calls(text: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    let mut remaining = text;
    while let Some(call) = ToolExecutor::parse_tool_call(remaining) {
        calls.push(call);
        if let Some(pos) = remaining.find("[TOOL_CALL:") {
            remaining = &remaining[pos + "[TOOL_CALL:".len()..];
            if let Some(end) = remaining.find(']') {
                remaining = &remaining[end + 1..];
            }
        } else {
            break;
        }
    }
    calls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tool_call() {
        let text = r#"[TOOL_CALL:{"name":"read_file","arguments":{"path":"/tmp/test.txt"}}]"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }
}
