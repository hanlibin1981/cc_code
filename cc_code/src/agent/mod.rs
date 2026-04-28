//! Agent 核心模块
//! 实现自主编程助手的大脑 - 推理循环和任务执行

mod task;
pub mod fork;
pub mod coordinator;

use crate::model::retry::{ApiError, RetryDecision, RetryHandler};
use crate::session::{MessageRole, Session, SessionManager, SessionState};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Agent 配置
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// 模型 API URL
    pub model_url: String,
    /// API Key
    pub api_key: String,
    /// 模型名称
    pub model_name: String,
    /// 系统提示词
    pub system_prompt: String,
    /// 最大输出 tokens
    pub max_output_tokens: u32,
    /// 最大推理深度（防止无限循环）
    pub max_reasoning_depth: u32,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model_url: "https://api.minimaxi.com/anthropic/v1/messages".into(),
            api_key: std::env::var("MINIMAX_API_KEY").unwrap_or_default(),
            model_name: "MiniMax-M2".into(),
            system_prompt: SYSTEM_PROMPT.into(),
            max_output_tokens: 8192,
            max_reasoning_depth: 20,
        }
    }
}

/// 系统提示词
const SYSTEM_PROMPT: &str = r#"你是 cc_code，一个专业的 AI 编程助手，基于 MiniMax M2 模型驱动。

你的职责：
1. 理解用户的编程任务需求
2. 将复杂任务拆解为具体可执行的步骤
3. 通过工具调用完成编码任务
4. 及时汇报进度和结果

工作流程：
1. 理解任务 → 2. 规划步骤 → 3. 必要时调用工具 → 4. 检查结果 → 5. 完成或继续

核心原则：
- 每次只执行一个工具调用，等待结果后再决定下一步
- 文件操作前先用 read_file 确认内容，再进行 edit_file 或 write_file
- Bash 命令要谨慎，危险操作（rm -rf、dd 等）必须拒绝
- 工具执行后必须分析结果，再继续下一步
- 复杂任务要分步骤完成，每步都要有明确的目标

重要环境信息：
- 运行平台：macOS（Apple Silicon）
- 包管理器：brew（如需安装库用 brew install）
- C++编译器：g++（版本可用 g++ --version 查看）
- C++标准：优先使用 C++11/14/17，不要用非常新的特性确保兼容性

工具调用格式（必须严格遵循）：
当需要调用工具时，在回复末尾添加一行：
[TOOL_CALL:{"name":"tool_name","arguments":{"param1":"value1","param2":"value2"}}]

可用工具及参数：
- read_file(path): 读取文件，path 为文件路径
- write_file(path, content): 写入文件，path 为路径，content 为内容
- edit_file(path, old_text, new_text): 编辑文件，old_text 必须是文件中真实存在的文本
- bash(command, timeout?): 执行命令，command 为命令字符串，可选 timeout 秒
- glob(pattern, cwd?): 搜索文件，pattern 为 glob 模式（如 **/*.rs），cwd 为搜索目录
- grep(pattern, paths?): 搜索内容，pattern 为搜索关键词，paths 为文件路径数组

重要限制：
- 每个回复只能包含一个工具调用（不多不少）
- 如果不需要工具，直接回复分析结果和任务进度
- 工具结果以 {tool: "name", result: "..."} 格式在下一轮提供
- 遇到错误要分析原因并重试或调整策略
"#;

/// Agent 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCallRequest>,
    pub session_id: uuid::Uuid,
    pub state: SessionState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    pub arguments: std::collections::HashMap<String, serde_json::Value>,
}

/// Agent 错误
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Session not found: {0}")]
    SessionNotFound(uuid::Uuid),
    #[error("Model API error: {0}")]
    ModelError(String),
    #[error("Session error: {0}")]
    SessionError(String),
}

/// Agent 主模块
pub struct Agent {
    config: AgentConfig,
    sessions: Arc<RwLock<SessionManager>>,
    http_client: reqwest::Client,
    retry_handler: RefCell<RetryHandler>,
}

impl Agent {
    pub fn new(config: AgentConfig) -> Self {
        Self::with_sessions(config, Arc::new(RwLock::new(SessionManager::new())))
    }

    pub fn with_sessions(config: AgentConfig, sessions: Arc<RwLock<SessionManager>>) -> Self {
        Self {
            config,
            sessions,
            http_client: reqwest::Client::new(),
            retry_handler: RefCell::new(RetryHandler::new()),
        }
    }

    pub async fn create_session(&self, cwd: std::path::PathBuf) -> uuid::Uuid {
        let session = {
            let mut manager = self.sessions.write().await;
            manager.create_session(cwd)
        };
        session.id
    }

    pub async fn list_sessions(&self) -> Vec<crate::session::SessionSummary> {
        let mut manager = self.sessions.write().await;
        manager.list_sessions()
    }

    pub async fn stop_session(&self, session_id: uuid::Uuid) -> Result<(), AgentError> {
        let mut manager = self.sessions.write().await;
        let session = manager
            .get_session_mut(&session_id)
            .ok_or_else(|| AgentError::SessionNotFound(session_id))?;
        session.set_state(SessionState::Stopped);
        Ok(())
    }

    /// 处理用户消息并生成响应（带推理循环）
    pub async fn process_message(
        &mut self,
        session_id: uuid::Uuid,
        message: String,
    ) -> Result<AgentResponse, AgentError> {
        // 推理深度计数（防止无限循环）
        let mut reasoning_depth = 0u32;
        let max_depth = self.config.max_reasoning_depth;

        // Step 1: 添加用户消息和待处理的工具结果
        let pending_tool_results = {
            let should_compact = {
                let manager = self.sessions.read().await;
                manager
                    .get_session(&session_id)
                    .map(|s| crate::session::memory::needs_compaction(s))
                    .unwrap_or(false)
            };

            let mut manager = self.sessions.write().await;
            let session = manager
                .get_session_mut(&session_id)
                .ok_or_else(|| AgentError::SessionNotFound(session_id))?;

            session.add_message(MessageRole::User, message.clone());
            if should_compact {
                crate::session::memory::compact_session(session);
            }
            session.drain_tool_results()
        };

        // Step 2: 推理循环
        loop {
            reasoning_depth += 1;

            // 获取 session 快照用于构建 prompt
            let session_snapshot = {
                let manager = self.sessions.read().await;
                manager
                    .get_session(&session_id)
                    .ok_or_else(|| AgentError::SessionNotFound(session_id))?
                    .clone()
            };

            // 构建 prompt（包含历史消息 + 最新工具结果）
            let prompt = self.build_prompt(&session_snapshot, pending_tool_results.clone());

            // 调用模型
            let response = self.call_model(&prompt).await?;

            // 解析响应
            let (text, tool_calls) = self.parse_response(&response);

            // 更新 session
            {
                let mut manager = self.sessions.write().await;
                let session = manager
                    .get_session_mut(&session_id)
                    .ok_or_else(|| AgentError::SessionNotFound(session_id))?;

                session.add_message(MessageRole::Assistant, text.clone());

                // 决定下一步
                if !tool_calls.is_empty() && reasoning_depth < max_depth {
                    // 还有工具要调用，继续循环
                    session.set_state(SessionState::WaitingTool);
                    return Ok(AgentResponse {
                        content: text,
                        tool_calls,
                        session_id,
                        state: SessionState::WaitingTool,
                    });
                } else {
                    // 无更多工具调用（或达到最大深度），结束
                    session.set_state(SessionState::Completed);
                    return Ok(AgentResponse {
                        content: if reasoning_depth >= max_depth {
                            format!(
                                "{}\n\n[推理深度达到上限 ({}), 请继续或开启新对话]",
                                text, max_depth
                            )
                        } else {
                            text
                        },
                        tool_calls,
                        session_id,
                        state: SessionState::Completed,
                    });
                }
            }
        }
    }

    /// 添加工具结果到 session（不继续推理，由 process_message 统一处理）
    pub async fn add_tool_result(
        &mut self,
        session_id: uuid::Uuid,
        tool_call_id: &str,
        tool_name: &str,
        result: String,
        is_error: bool,
    ) -> Result<(), AgentError> {
        let mut manager = self.sessions.write().await;
        let session = manager
            .get_session_mut(&session_id)
            .ok_or_else(|| AgentError::SessionNotFound(session_id))?;

        // 添加到 HashMap（按 tool_call_id）
        session.add_tool_result(tool_call_id, result.clone(), is_error);
        // 添加到 Vec（简化格式，供 drain）
        session.add_simple_tool_result(tool_name.into(), result.clone(), is_error);
        // 添加为 Tool 消息（对话历史）
        session.add_message(
            MessageRole::Tool,
            format!(
                "[{}] 结果: {}",
                tool_name,
                if is_error { format!("错误: {}", result) } else { result }
            ),
        );
        session.set_state(SessionState::Executing);
        Ok(())
    }

    /// 构建发送给模型的 prompt
    fn build_prompt(&self, session: &Session, tool_results: Vec<crate::session::SimpleToolResult>) -> String {
        let mut prompt = format!(
            "{}\n\n## 当前会话\n\n工作目录: {}\n\n## 对话历史:\n",
            self.config.system_prompt,
            session.cwd.display()
        );

        for msg in &session.messages {
            let role_str = match msg.role {
                MessageRole::User => "用户",
                MessageRole::Assistant => "助手",
                MessageRole::System => "系统",
                MessageRole::Tool => "工具",
            };
            prompt.push_str(&format!("\n[{}]\n{}\n", role_str, msg.content));
        }

        // 工具执行结果（来自 OpenClaw 执行后反馈）
        if !tool_results.is_empty() {
            prompt.push_str("\n## 工具执行结果:\n");
            for result in &tool_results {
                let prefix = if result.is_error { "错误" } else { "结果" };
                prompt.push_str(&format!("[{}] {}: {}\n", result.tool, prefix, result.result));
            }
        }
        // 旧的 tool_results (by ID)
        if !session.tool_results.is_empty() {
            prompt.push_str("\n## 待处理工具结果:\n");
            for (id, result) in &session.tool_results {
                prompt.push_str(&format!("- {}: {}\n", id, result.content));
            }
        }

        prompt.push_str("\n\n请继续完成任务或调用工具:");
        prompt
    }

    /// 调用模型 API（带重试逻辑）
    async fn call_model(&mut self, prompt: &str) -> Result<String, AgentError> {
        use std::io::Write as IoWrite;
        let start_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/cc_timing.log")
        {
            writeln!(&mut f, "[{}] START len={}", start_ms, prompt.len()).ok();
        }

        // Anthropic Messages API 格式
        #[derive(Serialize)]
        struct AnthropicRequest {
            model: String,
            messages: Vec<AnthropicMessage>,
            max_tokens: u32,
        }

        #[derive(Serialize)]
        struct AnthropicMessage {
            role: String,
            content: String,
        }

        #[derive(Deserialize)]
        struct AnthropicResponse {
            content: Vec<ContentBlock>,
        }

        #[derive(Deserialize)]
        #[serde(tag = "type")]
        enum ContentBlock {
            #[serde(rename = "text")]
            Text { text: String },
            #[serde(rename = "thinking")]
            Thinking { thinking: String },
            #[serde(rename = "tool_use")]
            #[allow(dead_code)]
            ToolUse { id: String, name: String, input: serde_json::Value },
        }

        let request = AnthropicRequest {
            model: self.config.model_name.clone(),
            messages: vec![AnthropicMessage {
                role: "user".into(),
                content: prompt.into(),
            }],
            max_tokens: self.config.max_output_tokens,
        };

        loop {
            let response = match self
                .http_client
                .post(&self.config.model_url)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("Content-Type", "application/json")
                .header("anthropic-version", "2023-06-01")
                .json(&request)
                .timeout(std::time::Duration::from_secs(120))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let api_err = ApiError::ConnectionError {
                        code: "REQUEST_FAILED".to_string(),
                        message: e.to_string(),
                    };
                    let decision = self.retry_handler.borrow_mut().get_decision(&api_err, Some("agent"));
                    match decision {
                        RetryDecision::Retry { delay_ms } => {
                            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                            continue;
                        }
                        _ => return Err(AgentError::ModelError(e.to_string())),
                    }
                }
            };
            let status = response.status();
            if !status.is_success() {
                let headers: Vec<(String, String)> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect();
                let body = response.text().await.unwrap_or_default();
                let api_err = RetryHandler::parse_http_error(status.as_u16(), &body, &headers);
                let decision = self.retry_handler.borrow_mut().get_decision(&api_err, Some("agent"));

                match decision {
                    RetryDecision::Retry { delay_ms } => {
                        let end_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open("/tmp/cc_timing.log")
                        {
                            writeln!(&mut f, "[{}] RETRY status={} delay={}ms", end_ms, status.as_u16(), delay_ms).ok();
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                        self.retry_handler.borrow_mut().reset();
                        continue;
                    }
                    RetryDecision::GiveUp => {
                        return Err(AgentError::ModelError(format!(
                            "HTTP {}: {} (after retries)",
                            status.as_u16(),
                            body
                        )));
                    }
                    _ => {
                        return Err(AgentError::ModelError(format!(
                            "HTTP {}: {}",
                            status.as_u16(),
                            body
                        )));
                    }
                }
            }

            let chat_response: AnthropicResponse = match response.json().await {
                Ok(c) => c,
                Err(e) => {
                    return Err(AgentError::ModelError(format!(
                        "Failed to parse response: {}",
                        e
                    )));
                }
            };

            // 从 Anthropic content 数组中提取文本
            let text = chat_response
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.clone()),
                    ContentBlock::Thinking { thinking } => Some(thinking.clone()),
                    ContentBlock::ToolUse { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n");

            let end_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/cc_timing.log")
            {
                writeln!(&mut f, "[{}] END len={}", end_ms, text.len()).ok();
            }

            return Ok(text);
        }
    }

    /// 解析模型响应，提取文本和工具调用
    /// 格式: [TOOL_CALL:{"name":"tool_name","arguments":{...}}]
    /// 使用括号平衡算法，支持任意层级的嵌套 JSON
    fn parse_response(&self, response: &str) -> (String, Vec<ToolCallRequest>) {
        let mut text = String::new();
        let mut tool_calls = Vec::new();

        let start_tag = "[TOOL_CALL";
        let mut search_start = 0;

        while let Some(tag_pos) = response[search_start..].find(start_tag) {
            let abs_tag_start = search_start + tag_pos;

            // 累积 [TOOL_CALL 之前的文本
            text.push_str(&response[search_start..abs_tag_start]);
            text.push('\n');

            // 找冒号后面第一个 {
            let brace_start = match response[abs_tag_start..].find('{') {
                Some(pos) => abs_tag_start + pos,
                None => break,
            };

            // 括号平衡，找到匹配的 }
            let mut depth = 0;
            let mut json_end = brace_start;
            for i in brace_start..response.len() {
                match response[i..=i].chars().next().unwrap_or('\0') {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            json_end = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }

            if depth != 0 {
                // 没找到匹配的括号，当作不是工具调用
                search_start = brace_start + 1;
                continue;
            }

            let json_str = &response[brace_start..=json_end];

            // 解析工具调用
            if let Ok(tool_call) = serde_json::from_str::<serde_json::Value>(json_str) {
                let id = uuid::Uuid::new_v4().to_string();
                let name = tool_call
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                let arguments: std::collections::HashMap<String, serde_json::Value> = tool_call
                    .get("arguments")
                    .and_then(|v| v.as_object())
                    .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();

                tool_calls.push(ToolCallRequest { id, name, arguments });
            }

            // 跳过整个 [TOOL_CALL:...] 标签
            if let Some(close_bracket) = response[json_end..].find(']') {
                search_start = json_end + close_bracket + 1;
            } else {
                search_start = json_end + 1;
            }
        }

        // 累积剩余文本
        text.push_str(&response[search_start..]);

        // 如果没有内联格式，尝试 markdown ```tool 块
        if tool_calls.is_empty() {
            let mut in_tool_block = false;
            let mut tool_json = String::new();
            let mut plain_text = String::new();

            for line in response.lines() {
                if line.trim() == "```tool" {
                    in_tool_block = true;
                    tool_json.clear();
                } else if line.trim() == "```" && in_tool_block {
                    in_tool_block = false;
                    if let Ok(tool_call) = serde_json::from_str::<serde_json::Value>(&tool_json) {
                        let id = uuid::Uuid::new_v4().to_string();
                        let name = tool_call
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();

                        let arguments: std::collections::HashMap<String, serde_json::Value> = tool_call
                            .get("arguments")
                            .and_then(|v| v.as_object())
                            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                            .unwrap_or_default();

                        tool_calls.push(ToolCallRequest { id, name, arguments });
                    }
                } else if in_tool_block {
                    tool_json.push_str(line);
                    tool_json.push('\n');
                } else {
                    plain_text.push_str(line);
                    plain_text.push('\n');
                }
            }

            if tool_calls.is_empty() {
                text = plain_text;
            }
        }

        (text.trim().to_string(), tool_calls)
    }
}
