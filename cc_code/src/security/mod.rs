//! 安全模块
//! Bash 命令安全验证

mod bash_guard;

pub use bash_guard::{BashGuard, CommandSafety};

/// 权限模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// 允许所有操作
    Allow,
    /// 询问危险操作
    Ask,
    /// 拒绝所有写入/Bash
    Deny,
}

impl Default for PermissionMode {
    fn default() -> Self {
        Self::Ask
    }
}
