//! 流式响应模块
//! 支持 Server-Sent Events (SSE) 流式输出
//! 
//! 流式事件类型：
//! - content: 内容片段
//! - tool_call: 工具调用
//! - done: 完成
//! - error: 错误

use serde::{Deserialize, Serialize};

/// Agent 流式配置
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// 是否启用流式
    pub enabled: bool,
    /// 流式事件通道容量
    pub channel_size: usize,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            channel_size: 100,
        }
    }
}

/// 流式事件类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamingEvent {
    /// 内容片段
    #[serde(rename = "content")]
    Content { text: String },
    /// 工具调用
    #[serde(rename = "tool_call")]
    ToolCall { name: String, arguments: serde_json::Value },
    /// 完成
    #[serde(rename = "done")]
    Done { total_tokens: usize, content: String, tool_calls: Vec<serde_json::Value> },
    /// 错误
    #[serde(rename = "error")]
    Error { message: String },
    /// 进度
    #[serde(rename = "progress")]
    Progress { progress: f32, message: String },
}

impl StreamingEvent {
    /// 转为 SSE 格式
    pub fn to_sse(&self) -> String {
        format!("data: {}\n\n", serde_json::to_string(self).unwrap_or_default())
    }
}

/// 流式响应累加器
/// 用于在流式接收过程中累积内容并检测工具调用
#[derive(Default)]
pub struct StreamingAccumulator {
    pub content: String,
    pub tool_calls: Vec<serde_json::Value>,
    pub total_tokens: usize,
    pub is_done: bool,
}


impl StreamingAccumulator {
    /// 创建新累加器
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加内容片段
    pub fn add_content(&mut self, text: &str) {
        self.content.push_str(text);
    }

    /// 设置 token 数
    pub fn set_tokens(&mut self, tokens: usize) {
        self.total_tokens = tokens;
    }

    /// 完成流式接收，提取工具调用
    pub fn finish(&mut self) {
        self.is_done = true;
        self.extract_tool_calls();
    }

    /// 提取内容中的工具调用
    pub fn extract_tool_calls(&mut self) {
        let content = std::mem::take(&mut self.content);
        let mut remaining = content.as_str();
        let mut extracted_text = String::new();

        while let Some(start) = remaining.find("[TOOL_CALL") {
            // 保留 [TOOL_CALL 之前的内容
            extracted_text.push_str(&remaining[..start]);

            if let Some(brace_pos) = remaining[start..].find('{') {
                let abs_brace = start + brace_pos;
                let mut depth = 0;
                let mut end = abs_brace;

                for i in abs_brace..remaining.len() {
                    match remaining.as_bytes()[i] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = i + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                if depth == 0 {
                    let json_str = &remaining[abs_brace..end];
                    if let Ok(tc) = serde_json::from_str::<serde_json::Value>(json_str) {
                        if tc.get("name").and_then(|v| v.as_str()).is_some() {
                            self.tool_calls.push(tc);
                        }
                    }

                    // 跳过到 ] 之后
                    if let Some(close_bracket) = remaining[end..].find(']') {
                        remaining = &remaining[end + close_bracket + 1..];
                        continue;
                    }
                }
            }

            // 如果没找到完整的 JSON，跳过 [
            extracted_text.push('[');
            remaining = &remaining[start + 1..];
        }

        // 剩余内容
        extracted_text.push_str(remaining);
        self.content = extracted_text;
    }

    /// 获取最终结果
    pub fn get_result(&self) -> StreamingResult {
        StreamingResult {
            content: self.content.clone(),
            tool_calls: self.tool_calls.clone(),
            total_tokens: self.total_tokens,
        }
    }
}

/// 流式最终结果
#[derive(Debug, Clone)]
pub struct StreamingResult {
    pub content: String,
    pub tool_calls: Vec<serde_json::Value>,
    pub total_tokens: usize,
}

/// DeepSeek SSE 行解析
pub fn parse_sse_line(line: &str) -> Option<String> {
    if line.starts_with("data:") {
        Some(line[5..].trim().to_string())
    } else {
        None
    }
}

/// DeepSeek 流式响应事件解析
#[derive(Debug, Deserialize, Default)]
pub struct DeepSeekStreamEvent {
    pub id: Option<String>,
    pub choices: Option<Vec<DeepSeekStreamChoice>>,
    #[serde(default)]
    pub usage: Option<StreamUsage>,
}

#[derive(Debug, Deserialize, Default)]
pub struct DeepSeekStreamChoice {
    #[serde(default)]
    pub delta: DeepSeekDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct DeepSeekDelta {
    #[serde(default)]
    pub content: Option<String>,
}

/// DeepSeek API Usage (stream)
#[derive(Debug, Deserialize, Default)]
pub struct StreamUsage {
    #[serde(rename = "completion_tokens", default)]
    pub completion_tokens: Option<usize>,
    #[serde(rename = "total_tokens", default)]
    pub total_tokens: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct Usage {
    #[serde(rename = "completion_tokens")]
    completion_tokens: Option<usize>,
    #[serde(rename = "total_tokens")]
    total_tokens: Option<usize>,
}

/// 进度通知
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ProgressNotification {
    pub progress: f32,
    pub message: String,
}

impl ProgressNotification {
    /// 创建新进度通知
    pub fn new(progress: f32, message: &str) -> Self {
        Self {
            progress,
            message: message.to_string(),
        }
    }

    /// 转为 SSE 格式
    pub fn to_sse(&self) -> String {
        format!(
            "data: {}\n\n",
            serde_json::to_string(&StreamingEvent::Progress {
                progress: self.progress,
                message: self.message.clone(),
            }).unwrap_or_default()
        )
    }
}

/// 流式状态
#[derive(Debug, Clone)]
pub enum StreamingState {
    /// 空闲
    Idle,
    /// 流式传输中
    Streaming,
    /// 已完成
    Completed,
    /// 错误(String),
    Error(String),
}

impl StreamingState {
    pub fn is_streaming(&self) -> bool {
        matches!(self, StreamingState::Streaming)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accumulator_extract_tool_call() {
        let mut acc = StreamingAccumulator::new();
        acc.add_content("Hello, let me ");
        acc.add_content("help you with this.\n[TOOL_CALL:{\"name\":\"bash\",\"arguments\":{\"command\":\"ls\"}}]");
        acc.finish();

        assert_eq!(acc.content, "Hello, let me help you with this.\n");
        assert_eq!(acc.tool_calls.len(), 1);
        assert_eq!(acc.tool_calls[0]["name"], "bash");
    }

    #[test]
    fn test_progress_to_sse() {
        let progress = ProgressNotification::new(0.5, "Processing...");
        let sse = progress.to_sse();
        assert!(sse.contains("\"type\":\"progress\""));
        assert!(sse.contains("0.5"));
        assert!(sse.contains("Processing"));
    }
}
