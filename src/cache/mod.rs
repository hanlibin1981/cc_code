//! 缓存模块 - 消息/结果缓存，支持内存和 Redis 后端

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

/// 缓存条目
struct CacheEntry<V> {
    value: V,
    expires_at: Option<Instant>,
}

impl<V> CacheEntry<V> {
    fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| Instant::now() > exp)
            .unwrap_or(false)
    }
}

/// 内存缓存
pub struct MemoryCache<K, V> {
    entries: RwLock<HashMap<K, CacheEntry<V>>>,
    default_ttl: Option<Duration>,
}

impl<K, V> MemoryCache<K, V>
where
    K: std::hash::Hash + Eq + Clone + std::fmt::Debug,
    V: Clone,
{
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            default_ttl: None,
        }
    }

    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            default_ttl: Some(ttl),
        }
    }

    /// 设置缓存（使用默认 TTL）
    pub async fn set(&self, key: K, value: V) {
        self.set_with_ttl(key, value, self.default_ttl).await;
    }

    /// 设置缓存（指定 TTL）
    pub async fn set_with_ttl(&self, key: K, value: V, ttl: Option<Duration>) {
        let expires_at = ttl.map(|d| Instant::now() + d);
        let entry = CacheEntry { value, expires_at };
        self.entries.write().await.insert(key, entry);
    }

    /// 获取缓存
    pub async fn get(&self, key: &K) -> Option<V> {
        let entries = self.entries.read().await;
        if let Some(entry) = entries.get(key) {
            if !entry.is_expired() {
                return Some(entry.value.clone());
            }
        }
        drop(entries);

        // 清理过期条目
        self.entries.write().await.remove(key);
        None
    }

    /// 删除缓存
    pub async fn delete(&self, key: &K) {
        self.entries.write().await.remove(key);
    }

    /// 清空所有缓存
    pub async fn clear(&self) {
        self.entries.write().await.clear();
    }

    /// 获取缓存数量
    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    /// 是否为空
    pub async fn is_empty(&self) -> bool {
        self.entries.read().await.is_empty()
    }

    /// 清理所有过期条目
    pub async fn cleanup_expired(&self) {
        let mut entries = self.entries.write().await;
        entries.retain(|_, entry| !entry.is_expired());
    }
}

impl<K, V> Default for MemoryCache<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

/// 消息缓存 - 专门用于缓存 AI 响应和工具结果
pub struct MessageCache {
    /// 工具结果缓存 (tool_call_id -> result)
    tool_results: MemoryCache<String, String>,
    /// AI 响应缓存 (cache_key -> response)
    responses: MemoryCache<String, CachedResponse>,
    /// 最大缓存条目数
    max_entries: usize,
}

#[derive(Clone)]
pub struct CachedResponse {
    pub content: String,
    pub model: String,
    pub cached_at: Instant,
}

impl MessageCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            tool_results: MemoryCache::with_ttl(Duration::from_secs(300)), // 5分钟
            responses: MemoryCache::with_ttl(Duration::from_secs(3600)), // 1小时
            max_entries,
        }
    }

    /// 缓存工具结果
    pub async fn cache_tool_result(&self, tool_call_id: &str, result: &str) {
        if self.tool_results.len().await >= self.max_entries {
            self.tool_results.cleanup_expired().await;
        }
        self.tool_results.set(tool_call_id.to_string(), result.to_string()).await;
    }

    /// 获取工具结果
    pub async fn get_tool_result(&self, tool_call_id: &str) -> Option<String> {
        self.tool_results.get(&tool_call_id.to_string()).await
    }

    /// 缓存 AI 响应
    pub async fn cache_response(&self, cache_key: &str, content: String, model: &str) {
        if self.responses.len().await >= self.max_entries {
            self.responses.cleanup_expired().await;
        }
        self.responses
            .set(
                cache_key.to_string(),
                CachedResponse {
                    content,
                    model: model.to_string(),
                    cached_at: Instant::now(),
                },
            )
            .await;
    }

    /// 获取 AI 响应
    pub async fn get_response(&self, cache_key: &str) -> Option<CachedResponse> {
        self.responses.get(&cache_key.to_string()).await
    }

    /// 生成缓存键
    pub fn make_cache_key(model: &str, messages: &[&str]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        model.hash(&mut hasher);
        for msg in messages {
            msg.hash(&mut hasher);
        }
        format!("{}_{:x}", model, hasher.finish())
    }
}

impl Default for MessageCache {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// 全局缓存实例
pub type SharedCache = Arc<MessageCache>;

/// 创建共享缓存
pub fn create_shared_cache(max_entries: usize) -> SharedCache {
    Arc::new(MessageCache::new(max_entries))
}
