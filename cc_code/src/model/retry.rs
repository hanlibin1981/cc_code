//! 重试 + 529 降载处理
//! 参考 Claude Code 的 withRetry.ts
//! 
//! 特性：
//! - 529 (Overloaded) 指数退避重试，前台请求最多 3 次
//! - 429 Too Many Requests 同样处理
//! - 网络错误（ECONNRESET）自动重试
//! - 重试时发送 progress 消息告知客户端

use serde::{Deserialize, Serialize};

/// 重试配置
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// 最大重试次数
    pub max_retries: usize,
    /// 最大 529 错误次数
    pub max_529_retries: usize,
    /// 基础延迟（毫秒）
    pub base_delay_ms: u64,
    /// 最大延迟（毫秒）
    pub max_delay_ms: u64,
    /// 是否启用
    pub enabled: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 10,
            max_529_retries: 3,
            base_delay_ms: 500,
            max_delay_ms: 32_000,
            enabled: true,
        }
    }
}

/// 重试状态
#[derive(Debug, Clone)]
pub struct RetryState {
    /// 当前尝试次数
    pub attempt: usize,
    /// 连续 529 错误次数
    pub consecutive_529_errors: usize,
    /// 上次错误
    pub last_error: Option<ApiError>,
}

impl Default for RetryState {
    fn default() -> Self {
        Self {
            attempt: 0,
            consecutive_529_errors: 0,
            last_error: None,
        }
    }
}

/// API 错误
#[derive(Debug, Clone)]
pub enum ApiError {
    /// 529 Overloaded
    Overloaded { message: String },
    /// 429 Too Many Requests
    TooManyRequests { retry_after_ms: Option<u64> },
    /// 连接错误
    ConnectionError { code: String, message: String },
    /// HTTP 错误
    HttpError { status: u16, message: String },
    /// 上下文窗口超限
    ContextWindowExceeded { input_tokens: usize, limit: usize },
    /// 未知错误
    Unknown { message: String },
}

impl ApiError {
    pub fn is_retryable(&self) -> bool {
        match self {
            ApiError::Overloaded { .. } => true,
            ApiError::TooManyRequests { .. } => true,
            ApiError::ConnectionError { code, .. } => {
                // ECONNRESET, EPIPE 等网络错误可以重试
                code == "ECONNRESET" || code == "EPIPE"
            }
            ApiError::HttpError { status, .. } => {
                matches!(status, 408 | 409 | 429 | 500..=599)
            }
            ApiError::ContextWindowExceeded { .. } => false,
            ApiError::Unknown { .. } => false,
        }
    }

    pub fn status_code(&self) -> Option<u16> {
        match self {
            ApiError::Overloaded { .. } => Some(529),
            ApiError::TooManyRequests { .. } => Some(429),
            ApiError::HttpError { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn is_529(&self) -> bool {
        matches!(self, ApiError::Overloaded { .. })
    }

    pub fn is_429(&self) -> bool {
        matches!(self, ApiError::TooManyRequests { .. })
    }

    pub fn is_context_overflow(&self) -> bool {
        matches!(self, ApiError::ContextWindowExceeded { .. })
    }
}

/// 重试决策
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryDecision {
    /// 立即返回，不重试
    Return,
    /// 重试
    Retry { delay_ms: u64 },
    /// 使用备用模型
    UseFallback { fallback_model: String },
    /// 不再重试
    GiveUp,
}

/// 重试器
pub struct RetryHandler {
    config: RetryConfig,
    state: RetryState,
    /// 前台查询源（需要重试 529）
    foreground_sources: std::collections::HashSet<String>,
}

impl Default for RetryHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl RetryHandler {
    pub fn new() -> Self {
        let mut foreground_sources = std::collections::HashSet::new();
        foreground_sources.insert("repl_main_thread".to_string());
        foreground_sources.insert("sdk".to_string());
        foreground_sources.insert("agent".to_string());
        foreground_sources.insert("compact".to_string());
        foreground_sources.insert("hook_agent".to_string());
        foreground_sources.insert("verification_agent".to_string());

        Self {
            config: RetryConfig::default(),
            state: RetryState::default(),
            foreground_sources,
        }
    }

    /// 配置
    pub fn with_config(mut self, config: RetryConfig) -> Self {
        self.config = config;
        self
    }

    /// 重置状态
    pub fn reset(&mut self) {
        self.state = RetryState::default();
    }

    /// 获取决策
    pub fn get_decision(
        &mut self,
        error: &ApiError,
        query_source: Option<&str>,
    ) -> RetryDecision {
        self.state.attempt += 1;
        self.state.last_error = Some(error.clone());

        // 不检查 self.config.enabled —— caller 决定是否启用重试

        // 跟踪连续 529
        if error.is_529() {
            self.state.consecutive_529_errors += 1;

            // 前台请求最多 3 次 529
            let is_foreground = query_source
                .map(|s| self.foreground_sources.contains(s))
                .unwrap_or(true);

            if is_foreground && self.state.consecutive_529_errors >= self.config.max_529_retries {
                return RetryDecision::GiveUp;
            }
        }

        // 检查是否是可重试错误
        if !error.is_retryable() {
            return RetryDecision::GiveUp;
        }

        // 计算延迟
        let delay_ms = self.calculate_delay(error);

        // 检查重试次数限制
        if self.state.attempt > self.config.max_retries {
            return RetryDecision::GiveUp;
        }

        RetryDecision::Retry { delay_ms }
    }

    /// 计算延迟
    fn calculate_delay(&self, error: &ApiError) -> u64 {
        // 如果有 Retry-After 头，使用它
        if let ApiError::TooManyRequests { retry_after_ms } = error {
            if let Some(ms) = retry_after_ms {
                return *ms;
            }
        }

        // 指数退避
        let base = self.config.base_delay_ms;
        let exponential = base * 2u64.pow((self.state.attempt - 1) as u32);
        let jitter = (exponential as f64 * 0.25 * rand_simple()) as u64;
        let delay = exponential + jitter;

        delay.min(self.config.max_delay_ms)
    }

    /// 检查是否应该触发备用模型
    pub fn should_use_fallback(&self, current_attempts: usize) -> bool {
        current_attempts >= self.config.max_529_retries
    }

    /// 解析 HTTP 错误
    pub fn parse_http_error(status: u16, message: &str, headers: &[(String, String)]) -> ApiError {
        match status {
            529 => ApiError::Overloaded {
                message: message.to_string(),
            },
            429 => {
                let retry_after_ms = headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
                    .and_then(|(_, v)| v.parse::<f64>().ok())
                    .map(|s| (s * 1000.0) as u64);

                ApiError::TooManyRequests { retry_after_ms }
            }
            400 => {
                // 检查是否是上下文窗口超限
                if message.contains("context limit") || message.contains("max_tokens") {
                    // 解析 token 数量
                    let re = regex::Regex::new(r"(\d+) \+ (\d+) > (\d+)").ok();
                    if let Some(caps) = re.and_then(|r| r.captures(message)) {
                        let input_tokens = caps.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                        let limit = caps.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                        return ApiError::ContextWindowExceeded { input_tokens, limit };
                    }
                }
                ApiError::HttpError {
                    status,
                    message: message.to_string(),
                }
            }
            401 | 403 => ApiError::HttpError {
                status,
                message: message.to_string(),
            },
            500..=599 => ApiError::HttpError {
                status,
                message: message.to_string(),
            },
            _ => ApiError::HttpError {
                status,
                message: message.to_string(),
            },
        }
    }

    /// 解析连接错误
    pub fn parse_connection_error(code: &str, message: &str) -> ApiError {
        ApiError::ConnectionError {
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}

/// 简单随机数（不引入 rand 依赖）
fn rand_simple() -> f64 {
    use std::time::Instant;
    let now = Instant::now();
    let nanos = now.elapsed().as_nanos();
    ((nanos % 1000) as f64) / 1000.0
}

/// Progress 消息生成
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryProgressMessage {
    pub r#type: String,
    pub subtype: String,
    pub error_type: String,
    pub delay_ms: u64,
    pub attempt: usize,
    pub max_retries: usize,
    pub message: String,
}

impl RetryProgressMessage {
    pub fn new(error: &ApiError, delay_ms: u64, attempt: usize, max_retries: usize) -> Self {
        let error_type = match error {
            ApiError::Overloaded { .. } => "overloaded".to_string(),
            ApiError::TooManyRequests { .. } => "rate_limit".to_string(),
            ApiError::ConnectionError { code, .. } => format!("connection_error_{}", code),
            ApiError::HttpError { status, .. } => format!("http_error_{}", status),
            ApiError::ContextWindowExceeded { .. } => "context_overflow".to_string(),
            ApiError::Unknown { .. } => "unknown".to_string(),
        };

        let message = match error {
            ApiError::Overloaded { message } => message.clone(),
            ApiError::TooManyRequests { retry_after_ms } => {
                if let Some(ms) = retry_after_ms {
                    format!("Rate limited. Retrying in {:.1}s...", *ms as f64 / 1000.0)
                } else {
                    "Rate limited. Retrying...".to_string()
                }
            }
            ApiError::ConnectionError { code, message: _ } => {
                format!("Connection error ({}). Retrying...", code)
            }
            ApiError::HttpError { status, message } => {
                format!("HTTP error {}: {}. Retrying...", status, message)
            }
            ApiError::ContextWindowExceeded { .. } => {
                "Context window exceeded. Cannot retry.".to_string()
            }
            ApiError::Unknown { message } => {
                format!("Error: {}. Retrying...", message)
            }
        };

        Self {
            r#type: "system".to_string(),
            subtype: "api_retry".to_string(),
            error_type,
            delay_ms,
            attempt,
            max_retries,
            message,
        }
    }
}

/// 错误信息工具函数
pub fn extract_error_message(error: &str) -> String {
    // 去掉常见的错误前缀
    let prefixes = [
        "API Error: ",
        "Error: ",
        "Anthropic error: ",
    ];

    for prefix in &prefixes {
        if error.starts_with(prefix) {
            return error[prefix.len()..].to_string();
        }
    }

    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_529_is_retryable() {
        let error = ApiError::Overloaded { message: "Overloaded".to_string() };
        assert!(error.is_retryable());
        assert!(error.is_529());
    }

    #[test]
    fn test_429_is_retryable() {
        let error = ApiError::TooManyRequests { retry_after_ms: None };
        assert!(error.is_retryable());
        assert!(error.is_429());
    }

    #[test]
    fn test_connection_error_econnreset() {
        let error = ApiError::ConnectionError {
            code: "ECONNRESET".to_string(),
            message: "Connection reset".to_string(),
        };
        assert!(error.is_retryable());
    }

    #[test]
    fn test_context_overflow_not_retryable() {
        let error = ApiError::ContextWindowExceeded {
            input_tokens: 100000,
            limit: 200000,
        };
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_retry_decision() {
        let mut handler = RetryHandler::new();
        
        let error = ApiError::Overloaded { message: "Overloaded".to_string() };
        let decision = handler.get_decision(&error, Some("sdk"));
        
        assert!(matches!(decision, RetryDecision::Retry { .. }));
    }

    #[test]
    fn test_max_529_retries() {
        let mut handler = RetryHandler::new();
        handler.config.max_529_retries = 3;
        
        for i in 0..3 {
            let error = ApiError::Overloaded { message: "Overloaded".to_string() };
            let decision = handler.get_decision(&error, Some("sdk"));
            if i < 2 {
                assert!(matches!(decision, RetryDecision::Retry { .. }));
            }
        }
    }
}
