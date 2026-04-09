//! Bash 命令安全验证
//! 参考 Claude Code 的 bashSecurity.ts，实现多层验证

use std::collections::HashSet;

/// 命令安全级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSafety {
    /// 安全，直接允许
    Allow,
    /// 危险，需要确认
    Dangerous,
    /// 极高危险，明确拒绝
    Deny,
    /// 需要用户确认
    Ask,
}

impl CommandSafety {
    pub fn is_allowed(&self) -> bool {
        matches!(self, CommandSafety::Allow)
    }

    pub fn is_dangerous(&self) -> bool {
        matches!(self, CommandSafety::Dangerous | CommandSafety::Deny)
    }

    pub fn message(&self) -> &str {
        match self {
            CommandSafety::Allow => "",
            CommandSafety::Dangerous => "危险命令",
            CommandSafety::Deny => "禁止执行的命令",
            CommandSafety::Ask => "需要用户确认",
        }
    }
}

/// Bash 命令安全守卫
#[derive(Debug, Clone)]
pub struct BashGuard {
    /// 允许列表
    allow_list: HashSet<String>,
    /// 拒绝列表
    deny_list: HashSet<String>,
    /// 是否启用沙箱
    sandbox_enabled: bool,
}

impl Default for BashGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl BashGuard {
    pub fn new() -> Self {
        Self {
            allow_list: HashSet::new(),
            deny_list: HashSet::new(),
            sandbox_enabled: true,
        }
    }

    /// 验证命令安全性
    pub fn validate(&self, command: &str) -> CommandSafety {
        let trimmed = command.trim();

        // 空命令
        if trimmed.is_empty() {
            return CommandSafety::Deny;
        }

        // 检查 deny 列表
        if self.is_deny_listed(trimmed) {
            return CommandSafety::Deny;
        }

        // 检查危险命令
        if self.is_dangerous(trimmed) {
            // 检查是否在 allow 列表中
            if self.allow_list.contains(trimmed) {
                return CommandSafety::Allow;
            }
            return CommandSafety::Dangerous;
        }

        // 检查 allow 列表
        if self.allow_list.contains(trimmed) {
            return CommandSafety::Allow;
        }

        // 检查路径穿越
        if self.contains_path_traversal(trimmed) {
            return CommandSafety::Dangerous;
        }

        // 检查 shell 注入
        if self.contains_injection(trimmed) {
            return CommandSafety::Dangerous;
        }

        CommandSafety::Allow
    }

    /// 添加到允许列表
    pub fn allow(&mut self, command: &str) {
        self.allow_list.insert(command.to_string());
    }

    /// 添加到拒绝列表
    pub fn deny(&mut self, command: &str) {
        self.deny_list.insert(command.to_string());
    }

    /// 启用/禁用沙箱
    pub fn set_sandbox(&mut self, enabled: bool) {
        self.sandbox_enabled = enabled;
    }

    /// 是否在拒绝列表中
    fn is_deny_listed(&self, command: &str) -> bool {
        // 直接匹配
        if self.deny_list.contains(command) {
            return true;
        }

        // 前缀匹配
        for deny_cmd in &self.deny_list {
            if command.starts_with(&format!("{} ", deny_cmd)) {
                return true;
            }
        }

        false
    }

    /// 检测危险命令
    fn is_dangerous(&self, command: &str) -> bool {
        // 灾难性删除命令
        if command == "rm -rf /" || command == "rm -rf /*" {
            return true;
        }

        // dd 命令（磁盘写入）
        if command.starts_with("dd ") && command.contains("of=") {
            return true;
        }

        // mkfs 命令（格式化）
        if command.starts_with("mkfs") {
            return true;
        }

        // 危险的后台进程
        if command.contains("> /dev/sd") || command.contains("> /dev/hd") {
            return true;
        }

        false
    }

    /// 检测路径穿越
    pub fn contains_path_traversal(&self, path: &str) -> bool {
        // ../ 路径穿越
        if path.contains("../") || path.contains("..\\") {
            return true;
        }

        // ~ 展开攻击（简单检测）
        if path.contains("~root") || path.contains("~admin") {
            return true;
        }

        false
    }

    /// 检测 shell 注入
    fn contains_injection(&self, command: &str) -> bool {
        // 多余的分号后跟危险命令
        // 比如 `; rm` 或 `&& rm`
        let parts: Vec<&str> = command
            .split(|c| c == ';' || c == '&' || c == '|')
            .collect();
        if parts.len() > 1 {
            let dangerous_suffixes = ["rm", "mv", "dd", "mkfs", ":()", "> /dev/"];
            for part in &parts[1..] {
                let trimmed = part.trim();
                for suffix in dangerous_suffixes {
                    if trimmed.starts_with(suffix) {
                        return true;
                    }
                }
            }
        }

        // $(...) 命令替换
        if command.contains("$(") && !command.starts_with("echo $(") {
            return true;
        }

        // 反引号命令替换
        if command.contains("`") && command.matches("`").count() >= 2 {
            return true;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allow_safe_commands() {
        let guard = BashGuard::new();
        assert!(guard.validate("ls").is_allowed());
        assert!(guard.validate("pwd").is_allowed());
        assert!(guard.validate("echo hello").is_allowed());
    }

    #[test]
    fn test_deny_catastrophic() {
        let guard = BashGuard::new();
        assert!(guard.validate("rm -rf /").is_dangerous());
        assert!(guard.validate("dd if=/dev/zero of=/dev/sda").is_dangerous());
    }

    #[test]
    fn test_path_traversal() {
        let guard = BashGuard::new();
        assert!(guard
            .validate("cat /etc/passwd../../../shadow")
            .is_dangerous());
    }

    #[test]
    fn test_injection() {
        let guard = BashGuard::new();
        assert!(guard.validate("ls; rm -rf /").is_dangerous());
        assert!(guard.validate("echo hello $(rm -rf /)").is_dangerous());
    }

    #[test]
    fn test_allow_list() {
        let mut guard = BashGuard::new();
        guard.deny("git push");
        assert!(guard.validate("git push").is_dangerous());
        guard.allow("git push");
        assert!(guard.validate("git push").is_allowed());
    }
}
