//! OpenClaw 配置读取模块
//! 支持从 openclaw.json 读取模型配置

use serde::Deserialize;
use std::path::PathBuf;

/// OpenClaw 配置结构（仅包含我们需要读取的部分）
#[derive(Debug, Clone, Deserialize)]
pub struct OpenClawConfig {
    #[serde(rename = "models")]
    pub models: Option<ModelsConfig>,
    #[serde(rename = "agents")]
    pub agents: Option<AgentsConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelsConfig {
    #[serde(rename = "providers")]
    pub providers: Option<std::collections::HashMap<String, ModelProvider>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelProvider {
    #[serde(rename = "baseUrl")]
    pub base_url: Option<String>,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    pub api: Option<String>,
    pub models: Option<Vec<ModelInfo>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "maxTokens")]
    pub max_tokens: Option<u32>,
    #[serde(rename = "contextWindow")]
    pub context_window: Option<u32>,
    pub cost: Option<ModelCost>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelCost {
    pub input: Option<f64>,
    pub output: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentsConfig {
    #[serde(rename = "defaults")]
    pub defaults: Option<AgentDefaults>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentDefaults {
    #[serde(rename = "model")]
    pub model: Option<ModelSelector>,
    #[serde(rename = "models")]
    pub models: Option<std::collections::HashMap<String, ModelAlias>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelSelector {
    #[serde(rename = "primary")]
    pub primary: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelAlias {
    pub alias: Option<String>,
}

/// 解析后的模型配置
#[derive(Debug, Clone)]
pub struct ResolvedModelConfig {
    pub provider: String,   // e.g. "deepseek"
    pub model_id: String,  // e.g. "deepseek-v4-pro"
    pub base_url: String,
    pub api_key: String,
    pub max_tokens: u32,
}

impl OpenClawConfig {
    /// 从默认路径加载配置
    pub fn load_default() -> Option<Self> {
        Self::load_from_path(&Self::default_config_path())
    }

    /// 加载指定路径的配置
    pub fn load_from_path(path: &PathBuf) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// 默认配置路径
    fn default_config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("openclaw")
            .join("openclaw.json")
    }

    /// 根据模型引用解析配置（如 "deepseek/deepseek-v4-pro"）
    pub fn resolve_model(&self, model_ref: &str) -> Option<ResolvedModelConfig> {
        let parts: Vec<&str> = model_ref.split('/').collect();
        if parts.len() != 2 {
            return None;
        }
        let (provider, model_id) = (parts[0], parts[1]);

        let providers = self.models.as_ref()?.providers.as_ref()?;
        let provider_config = providers.get(provider)?;

        let base_url = provider_config.base_url.clone()?;
        let api_key = provider_config.api_key.clone()?;

        // 查找模型的 max_tokens
        let max_tokens = provider_config
            .models
            .as_ref()
            .and_then(|models| {
                models
                    .iter()
                    .find(|m| m.id == model_id || m.name.as_deref() == Some(model_id))
            })
            .and_then(|m| m.max_tokens)
            .unwrap_or(8192);

        Some(ResolvedModelConfig {
            provider: provider.to_string(),
            model_id: model_id.to_string(),
            base_url,
            api_key,
            max_tokens,
        })
    }

    /// 获取默认模型配置
    pub fn default_model(&self) -> Option<ResolvedModelConfig> {
        let primary = self
            .agents
            .as_ref()?
            .defaults
            .as_ref()?
            .model
            .as_ref()?
            .primary
            .as_ref()?;
        self.resolve_model(primary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() {
        let json = r#"{
            "models": {
                "providers": {
                    "deepseek": {
                        "baseUrl": "https://api.deepseek.com",
                        "apiKey": "test-key",
                        "api": "anthropic-messages",
                        "models": [{"id": "deepseek-v4-pro", "maxTokens": 8192}]
                    }
                }
            },
            "agents": {
                "defaults": {
                    "model": {"primary": "deepseek/deepseek-v4-pro"}
                }
            }
        }"#;
        let config: OpenClawConfig = serde_json::from_str(json).unwrap();
        let resolved = config.resolve_model("deepseek/deepseek-v4-pro").unwrap();
        assert_eq!(resolved.provider, "deepseek");
        assert_eq!(resolved.model_id, "deepseek-v4-pro");
        assert_eq!(resolved.base_url, "https://api.deepseek.com");
        assert_eq!(resolved.api_key, "test-key");
    }
}
