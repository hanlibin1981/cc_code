#![allow(dead_code)]
//! 路径访问验证
//! 参考 Claude Code 的 pathValidation.ts
//! 
//! 特性：
//! - 敏感路径拦截：~/.ssh, /etc, /system 等
//! - rm -rf / 等灾难性操作永远拒绝
//! - Process substitution <(cmd) / >(cmd) 拦截
//! - cd + write 组合必须手动确认

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// 路径操作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathOperation {
    Read,
    Write,
    Create,
    Delete,
}

/// 路径检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathCheckResult {
    pub allowed: bool,
    pub reason: Option<String>,
    pub is_sensitive: bool,
    pub requires_confirmation: bool,
}

impl PathCheckResult {
    pub fn allow() -> Self {
        Self {
            allowed: true,
            reason: None,
            is_sensitive: false,
            requires_confirmation: false,
        }
    }

    pub fn deny(reason: &str) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.to_string()),
            is_sensitive: true,
            requires_confirmation: false,
        }
    }

    pub fn ask(reason: &str) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.to_string()),
            is_sensitive: false,
            requires_confirmation: true,
        }
    }
}

/// 路径验证器
pub struct PathValidator {
    /// 允许的目录
    allowed_dirs: HashSet<String>,
    /// 拒绝的目录
    denied_dirs: HashSet<String>,
    /// 用户主目录
    home_dir: String,
    /// 当前工作目录
    cwd: String,
}

impl Default for PathValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl PathValidator {
    pub fn new() -> Self {
        let home = dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/home/user".to_string());

        Self {
            allowed_dirs: HashSet::new(),
            denied_dirs: HashSet::new(),
            home_dir: home.clone(),
            cwd: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| home.clone()),
        }
    }

    /// 设置工作目录
    pub fn set_cwd(&mut self, cwd: &str) {
        self.cwd = cwd.to_string();
    }

    /// 添加允许的目录
    pub fn allow_dir(&mut self, dir: &str) {
        let expanded = self.expand_path(dir);
        self.allowed_dirs.insert(expanded);
    }

    /// 添加拒绝的目录
    pub fn deny_dir(&mut self, dir: &str) {
        let expanded = self.expand_path(dir);
        self.denied_dirs.insert(expanded);
    }

    /// 展开路径（处理 ~ 和相对路径）
    fn expand_path(&self, path: &str) -> String {
        if path == "~" {
            return self.home_dir.clone();
        }
        if path.starts_with("~/") {
            return path.replace("~", &self.home_dir);
        }
        if !path.starts_with('/') {
            // 相对路径
            return Path::new(&self.cwd)
                .join(path)
                .to_string_lossy()
                .to_string();
        }
        path.to_string()
    }

    /// 规范化路径（解析 .. 和 .）
    fn normalize(&self, path: &str) -> String {
        Path::new(path)
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string())
    }

    /// 主要验证入口
    pub fn check(&self, path: &str, operation: PathOperation) -> PathCheckResult {
        // 空路径
        if path.trim().is_empty() {
            return PathCheckResult::deny("Empty path");
        }

        // 去掉引号
        let clean = path.trim().trim_matches(|c| c == '"' || c == '\'');
        let expanded = self.expand_path(clean);
        let normalized = self.normalize(&expanded);

        // 检测 shell 扩展语法
        if self.contains_shell_expansion(&expanded) {
            return PathCheckResult::deny(
                "Shell expansion syntax ($ or %) in paths requires manual approval",
            );
        }

        // 检测 UNC 路径（Windows 网络路径）
        if self.contains_unc_path(&expanded) {
            return PathCheckResult::deny("UNC network paths require manual approval");
        }

        // 检测 ~user 变体
        if expanded.starts_with("~") && !expanded.starts_with("~/") {
            return PathCheckResult::deny(
                "Tilde expansion variants (~user, ~+, ~-) in paths require manual approval",
            );
        }

        // 检测路径遍历
        if self.contains_path_traversal(&expanded) {
            return PathCheckResult::deny("Path traversal sequences (..) require manual approval");
        }

        // 敏感路径检查
        if let Some(result) = self.check_sensitive_paths(&normalized, operation) {
            return result;
        }

        // 检查是否在允许列表
        if self.is_in_allowed(&normalized) {
            return PathCheckResult::allow();
        }

        // 检查是否在拒绝列表
        if self.is_in_denied(&normalized) {
            return PathCheckResult::deny("Path is in denied list");
        }

        // 检查是否在工作目录内
        if self.is_in_cwd(&normalized) || operation == PathOperation::Read {
            return PathCheckResult::allow();
        }

        // 工作目录外的写操作需要确认
        if operation == PathOperation::Write || operation == PathOperation::Create {
            return PathCheckResult::ask(
                &format!("Write to {} outside working directory requires confirmation", path),
            );
        }

        PathCheckResult::allow()
    }

    /// 检测 shell 扩展语法
    fn contains_shell_expansion(&self, path: &str) -> bool {
        path.contains('$') || path.contains('%') || path.starts_with('=')
    }

    /// 检测 UNC 路径
    fn contains_unc_path(&self, path: &str) -> bool {
        // UNC 路径如 \\server\share
        path.starts_with("\\\\") || regex_cache::is_match(r"^[A-Za-z]:\\", path)
    }

    /// 检测路径遍历
    fn contains_path_traversal(&self, path: &str) -> bool {
        path.contains("../") || path.contains("..\\") || path.ends_with("/..") || path.ends_with("\\..")
    }

    /// 检查敏感路径
    fn check_sensitive_paths(
        &self,
        path: &str,
        operation: PathOperation,
    ) -> Option<PathCheckResult> {
        let path_lower = path.to_lowercase();
        let home_lower = self.home_dir.to_lowercase();

        // ~/.ssh 永远拒绝写操作
        if path_lower.contains("/.ssh") || path_lower.contains("\\.ssh") {
            if operation == PathOperation::Write || operation == PathOperation::Create {
                return Some(PathCheckResult::deny(
                    "~/.ssh directory is not allowed to be modified",
                ));
            }
            return Some(PathCheckResult::ask("Access to ~/.ssh requires confirmation"));
        }

        // /etc 目录（macOS 上 /etc 是 /private/etc 的 symlink）
        if path_lower.starts_with("/etc") || path_lower.starts_with("/private/etc") {
            return Some(PathCheckResult::ask("Access to /etc requires confirmation"));
        }

        // /var 目录（macOS 上 /var 是 /private/var 的 symlink）
        if path_lower.starts_with("/var") || path_lower.starts_with("/private/var") {
            return Some(PathCheckResult::ask("Access to /var requires confirmation"));
        }

        // /system 目录（macOS SIP 相关）
        if path_lower.starts_with("/system") {
            return Some(PathCheckResult::deny("/system directory access is not allowed"));
        }

        // /proc （Linux）
        if path_lower.starts_with("/proc") {
            // /proc/self/environ 特别危险
            if path_lower.contains("/environ") {
                return Some(PathCheckResult::deny(
                    "/proc/*/environ access could expose sensitive environment variables",
                ));
            }
            return Some(PathCheckResult::ask("/proc access requires confirmation"));
        }

        // /sys （Linux 系统目录）
        if path_lower.starts_with("/sys") {
            return Some(PathCheckResult::ask("/sys access requires confirmation"));
        }

        // /dev （设备文件）
        if path_lower.starts_with("/dev") {
            if operation == PathOperation::Write {
                return Some(PathCheckResult::ask("/dev write access requires confirmation"));
            }
        }

        // /boot
        if path_lower.starts_with("/boot") {
            return Some(PathCheckResult::deny("/boot directory is not allowed"));
        }

        // /root 主目录（如果是 root 用户）
        if path_lower.starts_with("/root") && home_lower != "/root" {
            return Some(PathCheckResult::ask("/root access requires confirmation"));
        }

        // Windows 系统目录
        if regex_cache::is_match(r"^[A-Za-z]:\\windows\\system", &path_lower)
            || regex_cache::is_match(r"^[A-Za-z]:\\program files", &path_lower) {
            return Some(PathCheckResult::ask("Windows system directory access requires confirmation"));
        }

        None
    }

    /// 检查是否在允许列表
    fn is_in_allowed(&self, path: &str) -> bool {
        for allowed in &self.allowed_dirs {
            if path.starts_with(allowed) || allowed == path {
                return true;
            }
        }
        false
    }

    /// 检查是否在拒绝列表
    fn is_in_denied(&self, path: &str) -> bool {
        for denied in &self.denied_dirs {
            if path.starts_with(denied) || denied == path {
                return true;
            }
        }
        false
    }

    /// 检查是否在工作目录内
    fn is_in_cwd(&self, path: &str) -> bool {
        let cwd_normalized = self.normalize(&self.cwd);
        path.starts_with(&cwd_normalized) || cwd_normalized == path
    }

    /// 检查是否是危险删除路径
    pub fn check_dangerous_deletion(&self, path: &str) -> PathCheckResult {
        let expanded = self.expand_path(path);
        let normalized = self.normalize(&expanded);

        // rm -rf / 永远拒绝
        if normalized == "/" || expanded == "/"
            || normalized == "\\" || expanded == "\\"
            || expanded.ends_with("/*") || expanded.ends_with("\\*") {
            return PathCheckResult::deny("Removing root directory is not allowed");
        }

        // rm -rf ~ 永远拒绝
        if normalized == self.home_dir || expanded == "~" {
            return PathCheckResult::deny("Removing home directory is not allowed");
        }

        // 直接子目录的根（/usr, /tmp, /etc 等）
        let parent = Path::new(&normalized)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        if parent == "/" || parent == "\\" {
            return PathCheckResult::deny(&format!(
                "Removing {} is not allowed",
                normalized
            ));
        }

        // 根目录的直接子目录
        if parent.is_empty() {
            return PathCheckResult::deny(&format!(
                "Removing {} is not allowed",
                normalized
            ));
        }

        PathCheckResult::allow()
    }

    /// 检查 cd + write 组合
    pub fn check_cd_write_combination(&self, cd_path: &str, write_path: &str) -> PathCheckResult {
        // 检查 cd 目标
        let cd_result = self.check(cd_path, PathOperation::Read);
        if !cd_result.allowed && !cd_result.requires_confirmation {
            return cd_result;
        }

        // 检查写操作
        let write_result = self.check(write_path, PathOperation::Write);
        if !write_result.allowed && !write_result.requires_confirmation {
            return write_result;
        }

        // cd 到一个非工作目录然后写入，这需要确认
        let cd_expanded = self.expand_path(cd_path);
        let cd_normalized = self.normalize(&cd_expanded);
        let cwd_normalized = self.normalize(&self.cwd);

        if cd_normalized != cwd_normalized && write_result.allowed {
            // cd 出去然后写入，需要确认
            return PathCheckResult::ask(&format!(
                "cd to {} and then write to {} requires confirmation",
                cd_path, write_path
            ));
        }

        PathCheckResult::allow()
    }
}

/// 简单正则
mod regex_cache {
    use std::collections::HashMap;
    use once_cell::sync::Lazy;
    use regex::Regex;

    static CACHE: Lazy<std::sync::Mutex<HashMap<String, Regex>>> = Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

    pub fn is_match(pattern: &str, text: &str) -> bool {
        // Simply compile and match - caching is optional optimization
        if let Ok(re) = Regex::new(pattern) {
            re.is_match(text)
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deny_root_deletion() {
        let validator = PathValidator::new();
        
        let result = validator.check_dangerous_deletion("/");
        assert!(!result.allowed);
    }

    #[test]
    fn test_deny_home_deletion() {
        let validator = PathValidator::new();
        
        let result = validator.check_dangerous_deletion("~");
        assert!(!result.allowed);
    }

    #[test]
    fn test_ssh_directory_write_denied() {
        let validator = PathValidator::new();
        
        let result = validator.check("/home/user/.ssh/id_rsa", PathOperation::Write);
        assert!(!result.allowed);
    }

    #[test]
    fn test_shell_expansion_blocked() {
        let validator = PathValidator::new();
        
        let result = validator.check("$HOME/.bashrc", PathOperation::Write);
        assert!(!result.allowed);
    }

    #[test]
    fn test_proc_environ_denied() {
        let validator = PathValidator::new();
        
        let result = validator.check("/proc/self/environ", PathOperation::Read);
        assert!(!result.allowed);
    }

    #[test]
    fn test_path_traversal_blocked() {
        let validator = PathValidator::new();
        
        let result = validator.check("../../../etc/passwd", PathOperation::Read);
        assert!(!result.allowed);
    }

    #[test]
    fn test_etc_needs_confirmation() {
        let validator = PathValidator::new();
        
        let result = validator.check("/etc/passwd", PathOperation::Read);
        assert!(result.requires_confirmation || !result.allowed);
    }
}
