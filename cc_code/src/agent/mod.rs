//! Agent 核心模块
//! 实现自主编程助手的大脑 - 推理循环和任务执行

mod task;

pub use task::TaskPlanner;

use crate::session::memory::{compact_session, needs_compaction};
use crate::session::{MessageRole, Session, SessionManager, SessionState};
use serde::{Deserialize, Serialize};
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
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model_url: "https://api.minimaxi.chat/v1/text/chatcompletion_v2".into(),
            api_key: std::env::var("MINIMAX_API_KEY").unwrap_or_default(),
            model_name: "MiniMax-M2".into(),
            system_prompt: SYSTEM_PROMPT.into(),
            max_output_tokens: 8192,
        }
    }
}

/// 系统提示词
const SYSTEM_PROMPT: &str = r#"你是 cc_code，一个专业的编程开发助手。

你的职责：
1. 理解用户的编程任务需求
2. 将复杂任务拆解为具体步骤
3. 通过工具调用完成编码任务
4. 及时汇报进度和结果

可用工具（通过 MCP 协议调用）：
- read_file: 读取文件内容
- write_file: 写入文件内容
- edit_file: 编辑文件
- bash: 执行 Shell 命令
- glob: 文件搜索
- grep: 内容搜索

工作流程：
1. 理解任务 → 2. 规划步骤 → 3. 执行工具 → 4. 检查结果 → 5. 完成或继续

重要原则：
- 每次只执行一个工具调用，等待结果后再继续
- 文件操作前先读取确认内容
- Bash 命令要谨慎，特别是 rm mv 等危险操作
- 遇到错误要分析原因并调整策略
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
}

impl Agent {
    pub fn new(config: AgentConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(RwLock::new(SessionManager::new())),
            http_client: reqwest::Client::new(),
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
        let manager = self.sessions.read().await;
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

    /// 处理用户消息并生成响应
    pub async fn process_message(
        &self,
        session_id: uuid::Uuid,
        message: String,
    ) -> Result<AgentResponse, AgentError> {
        // 获取或创建 session
        let session = {
            let mut manager = self.sessions.write().await;
            let session = manager
                .get_session_mut(&session_id)
                .ok_or_else(|| AgentError::SessionNotFound(session_id))?;

            // 添加用户消息
            session.add_message(MessageRole::User, message.clone());
            session.set_state(SessionState::Executing);

            // 检查是否需要压缩
            if needs_compaction(session) {
                compact_session(session);
            }

            session.clone()
        };

        // 构建 prompt
        let prompt = self.build_prompt(&session);

        // 调用模型
        let response = self.call_model(&prompt).await?;

        // 更新 session
        {
            let mut manager = self.sessions.write().await;
            let session = manager
                .get_session_mut(&session_id)
                .ok_or_else(|| AgentError::SessionNotFound(session_id))?;

            // 解析响应中的工具调用
            let (text, tool_calls) = self.parse_response(&response);

            session.add_message(MessageRole::Assistant, text.clone());

            if !tool_calls.is_empty() {
                session.set_state(SessionState::WaitingTool);
            } else {
                session.set_state(SessionState::Completed);
            }

            Ok(AgentResponse {
                content: text,
                tool_calls,
                session_id,
                state: session.state,
            })
        }
    }

    /// 添加工具结果到 session
    pub async fn add_tool_result(
        &self,
        session_id: uuid::Uuid,
        tool_call_id: &str,
        tool_name: &str,
        result: String,
        is_error: bool,
    ) -> Result<AgentResponse, AgentError> {
        {
            let mut manager = self.sessions.write().await;
            let session = manager
                .get_session_mut(&session_id)
                .ok_or_else(|| AgentError::SessionNotFound(session_id))?;

            session.add_tool_result(tool_call_id, result.clone(), is_error);
            session.add_message(
                MessageRole::Tool,
                format!(
                    "[{}] 结果: {}",
                    tool_name,
                    if is_error {
                        format!("错误: {}", result)
                    } else {
                        result
                    }
                ),
            );
            session.set_state(SessionState::Executing);
        }

        // 继续推理
        let session = {
            let manager = self.sessions.read().await;
            manager
                .get_session(&session_id)
                .ok_or_else(|| AgentError::SessionNotFound(session_id))?
                .clone()
        };

        let prompt = self.build_prompt(&session);
        let response = self.call_model(&prompt).await?;

        let (text, tool_calls) = self.parse_response(&response);

        {
            let mut manager = self.sessions.write().await;
            let session = manager
                .get_session_mut(&session_id)
                .ok_or_else(|| AgentError::SessionNotFound(session_id))?;

            session.add_message(MessageRole::Assistant, text.clone());

            if !tool_calls.is_empty() {
                session.set_state(SessionState::WaitingTool);
            } else {
                session.set_state(SessionState::Completed);
            }

            return Ok(AgentResponse {
                content: text,
                tool_calls,
                session_id,
                state: session.state,
            });
        }
    }

    /// 构建发送给模型的 prompt
    fn build_prompt(&self, session: &Session) -> String {
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

        // 如果有未处理的工具结果
        if !session.tool_results.is_empty() {
            prompt.push_str("\n## 待处理工具结果:\n");
            for (id, result) in &session.tool_results {
                prompt.push_str(&format!("- {}: {}\n", id, result.content));
            }
        }

        prompt.push_str("\n\n请继续完成任务或调用工具:");
        prompt
    }

    /// 调用模型 API
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
            choices: Vec<Choice>,
        }

        #[derive(Deserialize)]
        struct Choice {
            message: ResponseMessage,
        }

        #[derive(Deserialize)]
        struct ResponseMessage {
            content: String,
        }

        let request = ChatRequest {
            model: self.config.model_name.clone(),
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

        Ok(chat_response.choices[0].message.content.clone())
    }

    /// 解析模型响应，提取文本和工具调用
    fn parse_response(&self, response: &str) -> (String, Vec<ToolCallRequest>) {
        // 简单解析：查找 ```tool 块
        // 格式: ```tool
        // {"name": "read_file", "arguments": {"path": "..."}}
        // ```

        let mut text = String::new();
        let mut tool_calls = Vec::new();
        let mut in_tool_block = false;
        let mut tool_json = String::new();

        for line in response.lines() {
            if line.trim() == "```tool" {
                in_tool_block = true;
                tool_json.clear();
            } else if line.trim() == "```" && in_tool_block {
                in_tool_block = false;
                // 解析工具调用 JSON
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

                    tool_calls.push(ToolCallRequest {
                        id,
                        name,
                        arguments,
                    });
                }
            } else if in_tool_block {
                tool_json.push_str(line);
                tool_json.push('\n');
            } else {
                text.push_str(line);
                text.push('\n');
            }
        }

        (text.trim().to_string(), tool_calls)
    }
}
