//! Agent 核心模块
//! 实现自主编程助手的大脑 - 推理循环和任务执行
//! 
//! 特性：
//! - 多轮对话上下文（真正的历史消息）
//! - Anthropic Messages API 格式
//! - 自动上下文压缩
//! - Fork 子Agent 支持（框架已就绪）

#[allow(unused)]
pub mod fork;
#[allow(unused)]
pub mod coordinator;
pub mod streaming;

use crate::config::ResolvedModelConfig;
use crate::session::{MessageRole, Session, SessionManager, SessionState};
use crate::agent::streaming::{StreamingAccumulator, StreamingConfig, StreamingState, DeepSeekStreamEvent};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use futures_util::{TryStreamExt, AsyncBufReadExt};

/// Agent 配置
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// 模型 API URL
    pub model_url: String,
    /// API Key
    pub api_key: String,
    /// 模型引用（如 "deepseek/deepseek-v4-pro"）
    pub model_ref: String,
    /// 模型 ID（不含 provider 前缀）
    pub model_id: String,
    /// 系统提示词
    pub system_prompt: String,
    /// 最大输出 tokens
    pub max_output_tokens: u32,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model_url: "https://api.deepseek.com/v1/chat/completions".into(),
            api_key: std::env::var("DEEPSEEK_API_KEY").unwrap_or_default(),
            model_ref: "deepseek/deepseek-v4-pro".into(),
            model_id: "deepseek-v4-pro".into(),
            system_prompt: SYSTEM_PROMPT.into(),
            max_output_tokens: 8192,
        }
    }
}

impl AgentConfig {
    /// 从 OpenClaw 配置加载
    pub fn from_openclaw_config(
        resolved: &ResolvedModelConfig,
        model_ref: &str,
        system_prompt: Option<String>,
    ) -> Self {
        Self {
            model_url: format!("{}/v1/chat/completions", resolved.base_url.trim_end_matches('/')),
            api_key: resolved.api_key.clone(),
            model_ref: model_ref.to_string(),
            model_id: resolved.model_id.clone(),
            system_prompt: system_prompt.unwrap_or_else(|| SYSTEM_PROMPT.into()),
            max_output_tokens: resolved.max_tokens,
        }
    }
}

/// 系统提示词
const SYSTEM_PROMPT: &str = r#"你是 cc_code，一个专业的 AI 编程助手，基于 DeepSeek V4 Pro 模型驱动。

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
    #[error("Context too long: {0}")]
    ContextTooLong(usize),
}

/// Agent 主模块
pub struct Agent {
    config: AgentConfig,
    sessions: Arc<RwLock<SessionManager>>,
    http_client: reqwest::Client,
}

impl Agent {
    #[allow(unused)] pub fn new(config: AgentConfig) -> Self {
        Self::with_sessions(config, Arc::new(RwLock::new(SessionManager::new())))
    }

    pub fn with_sessions(config: AgentConfig, sessions: Arc<RwLock<SessionManager>>) -> Self {
        Self {
            config,
            sessions,
            http_client: reqwest::Client::new(),
        }
    }

    /// 处理用户消息并生成响应（多轮对话版本）
    pub async fn process_message(
        &self,
        session_id: uuid::Uuid,
        message: String,
    ) -> Result<AgentResponse, AgentError> {
        // 检查是否需要压缩
        let should_compact = {
            let manager = self.sessions.read().await;
            manager
                .get_session(&session_id)
                .map(crate::session::memory::needs_compaction)
                .unwrap_or(false)
        };

        // 获取 tool_results 并添加用户消息
        let tool_results = {
            let mut manager = self.sessions.write().await;
            let session = manager
                .get_session_mut(&session_id)
                .ok_or(AgentError::SessionNotFound(session_id))?;

            session.add_message(MessageRole::User, message.clone());

            if should_compact {
                crate::session::memory::compact_session(session);
            }

            session.drain_tool_results()
        };

        // 获取 session 用于构建消息历史
        let session = {
            let manager = self.sessions.read().await;
            manager
                .get_session(&session_id)
                .ok_or(AgentError::SessionNotFound(session_id))?
                .clone()
        };

        // 构建 Anthropic 格式的消息列表
        let messages = self.build_messages(&session, tool_results);

        // 调用模型（多轮对话格式）
        let response_text = self.call_model_multi_turn(&messages).await?;

        // 解析响应中的工具调用
        let (text, tool_calls) = self.parse_response(&response_text);

        // 更新 session
        {
            let mut manager = self.sessions.write().await;
            let session = manager
                .get_session_mut(&session_id)
                .ok_or(AgentError::SessionNotFound(session_id))?;

            session.add_message(MessageRole::Assistant, text.clone());

            if !tool_calls.is_empty() {
                session.set_state(SessionState::WaitingTool);
            } else {
                session.set_state(SessionState::Completed);
            }
        }

        Ok(AgentResponse {
            content: text,
            tool_calls,
            session_id,
            state: SessionState::Idle,
        })
    }

    /// 处理用户消息并生成流式响应
    /// 返回一个包含内容片段和工具调用的累积响应
    pub async fn process_message_streaming(
        &self,
        session_id: uuid::Uuid,
        message: String,
    ) -> Result<(String, Vec<ToolCallRequest>), AgentError> {
        // 检查是否需要压缩
        let should_compact = {
            let manager = self.sessions.read().await;
            manager
                .get_session(&session_id)
                .map(crate::session::memory::needs_compaction)
                .unwrap_or(false)
        };

        // 获取 tool_results 并添加用户消息
        let tool_results = {
            let mut manager = self.sessions.write().await;
            let session = manager
                .get_session_mut(&session_id)
                .ok_or(AgentError::SessionNotFound(session_id))?;

            session.add_message(MessageRole::User, message.clone());

            if should_compact {
                crate::session::memory::compact_session(session);
            }

            session.drain_tool_results()
        };

        // 获取 session 用于构建消息历史
        let session = {
            let manager = self.sessions.read().await;
            manager
                .get_session(&session_id)
                .ok_or(AgentError::SessionNotFound(session_id))?
                .clone()
        };

        // 构建 Anthropic 格式的消息列表
        let messages = self.build_messages(&session, tool_results);

        // 使用流式配置
        let streaming_config = StreamingConfig::default();

        // 调用模型（流式版本）
        let (accumulator, _state) = self
            .call_model_streaming(&messages, &streaming_config)
            .await?;

        let result = accumulator.get_result();

        // 解析响应中的工具调用
        let tool_calls: Vec<ToolCallRequest> = result
            .tool_calls
            .into_iter()
            .filter_map(|tc| {
                let name = tc.get("name")?.as_str()?.to_string();
                let arguments: std::collections::HashMap<String, serde_json::Value> = tc
                    .get("arguments")?
                    .as_object()?
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                Some(ToolCallRequest {
                    id: uuid::Uuid::new_v4().to_string(),
                    name,
                    arguments,
                })
            })
            .collect();

        // 更新 session
        {
            let mut manager = self.sessions.write().await;
            let session = manager
                .get_session_mut(&session_id)
                .ok_or(AgentError::SessionNotFound(session_id))?;

            session.add_message(MessageRole::Assistant, result.content.clone());

            if !tool_calls.is_empty() {
                session.set_state(SessionState::WaitingTool);
            } else {
                session.set_state(SessionState::Completed);
            }
        }

        Ok((result.content, tool_calls))
    }

    /// 构建 Anthropic Messages API 格式的消息列表
    fn build_messages(
        &self,
        session: &Session,
        tool_results: Vec<crate::session::SimpleToolResult>,
    ) -> Vec<ChatMessage> {
        let mut messages = Vec::new();

        // 系统消息（作为第一条消息）
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: self.config.system_prompt.clone(),
        });

        // 对话历史
        for msg in &session.messages {
            if msg.role == MessageRole::System {
                continue; // 系统消息已在上面处理
            }
            let role_str = match msg.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "user", // 工具结果作为用户消息
                MessageRole::System => continue,
            };

            let mut content = msg.content.clone();

            // 如果有工具调用，附加到内容
            if let Some(ref calls) = msg.tool_calls {
                for tc in calls {
                    if let Ok(args_json) = serde_json::to_string(&tc.arguments) {
                        content.push_str(&format!(
                            "\n[TOOL_CALL: {{\"name\": \"{}\", \"arguments\": {}}}]",
                            tc.name, args_json
                        ));
                    }
                }
            }

            messages.push(ChatMessage {
                role: role_str.to_string(),
                content,
            });
        }

        // 添加工具执行结果（如果有）
        if !tool_results.is_empty() {
            let mut results_text = String::from("\n## 工具执行结果:\n");
            for result in &tool_results {
                let prefix = if result.is_error { "错误" } else { "结果" };
                results_text.push_str(&format!(
                    "- [{}] {}: {}\n",
                    result.tool, prefix, result.result
                ));
            }
            results_text.push_str("\n请继续完成任务或调用下一个工具：");

            messages.push(ChatMessage {
                role: "user".to_string(),
                content: results_text,
            });
        }

        messages
    }

    /// 调用模型（多轮对话格式，Anthropic Messages API）
    async fn call_model_multi_turn(
        &self,
        messages: &[ChatMessage],
    ) -> Result<String, AgentError> {
        // 检查上下文长度
        let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
        const MAX_CONTEXT: usize = 100_000;
        if total_chars > MAX_CONTEXT {
            return Err(AgentError::ContextTooLong(total_chars));
        }

        #[derive(Serialize)]
        struct ChatRequest<'a> {
            model: &'a str,
            messages: &'a [ChatMessage],
            max_tokens: u32,
        }

        #[derive(Deserialize)]
        struct ChatResponse {
            choices: Vec<ChatChoice>,
        }

        #[derive(Deserialize)]
        struct ChatChoice {
            message: ChatMessageContent,
        }

        #[derive(Deserialize)]
        struct ChatMessageContent {
            content: String,
        }

        let request = ChatRequest {
            model: &self.config.model_id,
            messages,
            max_tokens: self.config.max_output_tokens,
        };

        let response = self
            .http_client
            .post(&self.config.model_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| AgentError::ModelError(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AgentError::ModelError(format!(
                "HTTP {}: {}",
                status.as_u16(),
                body
            )));
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| AgentError::ModelError(e.to_string()))?;

        let text = chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(text)
    }

    /// 调用模型（流式版本）
    /// 返回累积器，在外部逐步添加内容
    pub async fn call_model_streaming(
        &self,
        messages: &[ChatMessage],
        _config: &StreamingConfig,
    ) -> Result<(StreamingAccumulator, StreamingState), AgentError> {
        // 检查上下文长度
        let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
        const MAX_CONTEXT: usize = 100_000;
        if total_chars > MAX_CONTEXT {
            return Err(AgentError::ContextTooLong(total_chars));
        }

        #[derive(Serialize)]
        struct StreamRequest<'a> {
            model: &'a str,
            messages: &'a [ChatMessage],
            max_tokens: u32,
        }


        let request = StreamRequest {
            model: &self.config.model_id,
            messages,
            max_tokens: self.config.max_output_tokens,
        };


        let response = self
            .http_client
            .post(&self.config.model_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await
            .map_err(|e| AgentError::ModelError(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AgentError::ModelError(format!(
                "HTTP {}: {}",
                status.as_u16(),
                body
            )));
        }

        // 流式读取响应
        let mut accumulator = StreamingAccumulator::new();

        let mut reader = response
            .bytes_stream()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
            .into_async_read();


        let mut line_buf = Vec::new();
        loop {
            match reader.read_until(b'\n', &mut line_buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let line_bytes = line_buf[..n].to_vec();
                    line_buf.clear();
                    let line = String::from_utf8_lossy(&line_bytes);

                    // 解析 SSE 行
                    if let Some(data) = parse_sse_line(&line) {
                        if data == "[DONE]" {
                            break;
                        }


                        // 解析 DeepSeek 流式事件
                        if let Ok(event) = serde_json::from_str::<DeepSeekStreamEvent>(&data) {
                            if let Some(choices) = event.choices {
                                for choice in choices {
                                    if let Some(content) = choice.delta.content {
                                        accumulator.add_content(&content);
                                    }
                                }
                            }

                            // 更新 token 统计
                            if let Some(usage) = event.usage {
                                if let Some(tokens) = usage.total_tokens {
                                    accumulator.set_tokens(tokens);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    accumulator.finish();
                    return Ok((accumulator, StreamingState::Error(err_msg)));
                }
            }
        }

        accumulator.finish();
        Ok((accumulator, StreamingState::Completed))
    }

    /// 调用模型（单轮 prompt 格式，兼容旧接口）
    #[allow(unused)]
    async fn call_model(&self, prompt: &str) -> Result<String, AgentError> {
        #[derive(Serialize)]
        struct ChatRequest {
            model: String,
            messages: Vec<ChatMessage>,
            max_tokens: u32,
        }

        #[derive(Serialize)]
        struct ChatMessage {
            role: String,
            content: String,
        }

        #[derive(Deserialize)]
        struct ChatResponse {
            choices: Vec<ChatChoice>,
        }

        #[derive(Deserialize)]
        struct ChatChoice {
            message: ChatMessageContent,
        }

        #[derive(Deserialize)]
        struct ChatMessageContent {
            content: String,
        }

        let request = ChatRequest {
            model: self.config.model_id.clone(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: prompt.into(),
            }],
            max_tokens: self.config.max_output_tokens,
        };

        let response = self
            .http_client
            .post(&self.config.model_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| AgentError::ModelError(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AgentError::ModelError(format!(
                "HTTP {}: {}",
                status.as_u16(),
                body
            )));
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| AgentError::ModelError(e.to_string()))?;

        Ok(chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default())
    }

    /// 解析模型响应，提取文本和工具调用
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
                search_start = brace_start + 1;
                continue;
            }

            let json_str = &response[brace_start..=json_end];

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

            if let Some(close_bracket) = response[json_end..].find(']') {
                search_start = json_end + close_bracket + 1;
            } else {
                search_start = json_end + 1;
            }
        }

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

/// 聊天消息结构（用于 Anthropic Messages API）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}


/// DeepSeek SSE 行解析
fn parse_sse_line(line: &str) -> Option<String> {
    if line.starts_with("data:") {
        Some(line[5..].trim().to_string())
    } else {
        None
    }
}

