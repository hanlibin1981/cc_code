//! 模型接口抽象
//! 支持多种模型提供商

pub mod retry;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// 模型错误
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
}

/// 模型响应
#[derive(Debug, Clone)]
pub struct ModelResponse {
    pub content: String,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// 模型trait
#[async_trait]
pub trait Model: Send + Sync {
    /// 生成响应
    async fn generate(&self, prompt: &str) -> Result<ModelResponse, ModelError>;
}

/// DeepSeek API 模型
#[derive(Debug, Clone)]
pub struct DeepSeekModel {
    api_key: String,
    model_id: String,
    base_url: String,
    http_client: reqwest::Client,
}

impl DeepSeekModel {
    pub fn new(api_key: String, model_id: String) -> Self {
        Self {
            api_key,
            model_id,
            base_url: "https://api.deepseek.com/v1".into(),
            http_client: reqwest::Client::new(),
        }
    }

    pub fn with_url(api_key: String, model_id: String, base_url: String) -> Self {
        Self {
            api_key,
            model_id,
            base_url,
            http_client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Model for DeepSeekModel {
    async fn generate(&self, prompt: &str) -> Result<ModelResponse, ModelError> {
        #[derive(Serialize)]
        struct ChatRequest {
            model: String,
            messages: Vec<ChatMessage>,
            max_tokens: u32,
            stream: bool,
        }

        #[derive(Serialize)]
        struct ChatMessage {
            role: String,
            content: String,
        }

        #[derive(Deserialize)]
        struct ChatResponse {
            choices: Vec<Choice>,
            #[serde(default)]
            usage: Option<Usage>,
        }

        #[derive(Deserialize)]
        struct Choice {
            message: ResponseMessage,
        }

        #[derive(Deserialize)]
        struct ResponseMessage {
            content: String,
        }

        #[derive(Deserialize)]
        struct Usage {
            #[serde(rename = "prompt_tokens")]
            prompt_tokens: Option<u32>,
            #[serde(rename = "completion_tokens")]
            completion_tokens: Option<u32>,
            #[serde(rename = "total_tokens")]
            total_tokens: Option<u32>,
        }

        let request = ChatRequest {
            model: self.model_id.clone(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: prompt.into(),
            }],
            max_tokens: 8192,
            stream: false,
        };

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| ModelError::NetworkError(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ModelError::ApiError(format!(
                "HTTP {}: {}",
                status.as_u16(),
                body
            )));
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| ModelError::ParseError(e.to_string()))?;

        let usage = chat_response.usage.map(|u| TokenUsage {
            prompt_tokens: u.prompt_tokens.unwrap_or(0),
            completion_tokens: u.completion_tokens.unwrap_or(0),
            total_tokens: u.total_tokens.unwrap_or(0),
        });

        Ok(ModelResponse {
            content: chat_response.choices[0].message.content.clone(),
            usage,
        })
    }
}

/// MiniMax API 模型（保留兼容）
#[derive(Debug, Clone)]
pub struct MiniMaxModel {
    api_key: String,
    model_name: String,
    base_url: String,
    http_client: reqwest::Client,
}

impl MiniMaxModel {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            model_name: "MiniMax-M2".into(),
            base_url: "https://api.minimaxi.chat/v1/text/chatcompletion_v2".into(),
            http_client: reqwest::Client::new(),
        }
    }

    pub fn with_config(api_key: String, model_name: String, base_url: String) -> Self {
        Self {
            api_key,
            model_name,
            base_url,
            http_client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Model for MiniMaxModel {
    async fn generate(&self, prompt: &str) -> Result<ModelResponse, ModelError> {
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
            #[serde(default)]
            usage: Option<Usage>,
        }

        #[derive(Deserialize)]
        struct Choice {
            message: ResponseMessage,
        }

        #[derive(Deserialize)]
        struct ResponseMessage {
            content: String,
        }

        #[derive(Deserialize)]
        struct Usage {
            #[serde(rename = "prompt_tokens")]
            prompt_tokens: Option<u32>,
            #[serde(rename = "completion_tokens")]
            completion_tokens: Option<u32>,
            #[serde(rename = "total_tokens")]
            total_tokens: Option<u32>,
        }

        let request = ChatRequest {
            model: self.model_name.clone(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: prompt.into(),
            }],
            max_tokens: 8192,
        };

        let response = self
            .http_client
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| ModelError::NetworkError(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ModelError::ApiError(format!(
                "HTTP {}: {}",
                status.as_u16(),
                body
            )));
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| ModelError::ParseError(e.to_string()))?;

        let usage = chat_response.usage.map(|u| TokenUsage {
            prompt_tokens: u.prompt_tokens.unwrap_or(0),
            completion_tokens: u.completion_tokens.unwrap_or(0),
            total_tokens: u.total_tokens.unwrap_or(0),
        });

        Ok(ModelResponse {
            content: chat_response.choices[0].message.content.clone(),
            usage,
        })
    }
}
