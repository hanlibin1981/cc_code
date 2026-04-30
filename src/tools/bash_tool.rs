//! Bash 命令执行工具

use crate::tools::ToolExecutionResult;
use std::collections::HashSet;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

pub struct BashTool {
    dangerous_commands: HashSet<&'static str>,
    allowed_commands: Option<HashSet<&'static str>>,
}

impl BashTool {
    pub fn new() -> Self {
        let mut dangerous = HashSet::new();
        dangerous.insert("rm -rf /");
        dangerous.insert("rm -rf /*");
        dangerous.insert("dd if=/dev/zero of=/dev/sda");
        dangerous.insert("mkfs");
        dangerous.insert("fdisk");
        dangerous.insert("chmod -R 777 /");
        dangerous.insert(":(){:|:&};:");
        Self {
            dangerous_commands: dangerous,
            allowed_commands: None,
        }
    }

    pub fn with_whitelist(mut self, commands: HashSet<&'static str>) -> Self {
        self.allowed_commands = Some(commands);
        self
    }

    pub async fn execute(&self, command: &str, timeout_ms: u64) -> ToolExecutionResult {
        if let Some(reason) = self.validate_command(command) {
            return ToolExecutionResult::err(format!("Command blocked: {}", reason));
        }

        match self.run_command(command, timeout_ms).await {
            Ok((stdout, stderr, exit_code)) => {
                let output = if stdout.is_empty() && !stderr.is_empty() {
                    format!("STDERR:\n{}", stderr)
                } else if stderr.is_empty() {
                    stdout
                } else {
                    format!("STDOUT:\n{}\n\nSTDERR:\n{}", stdout, stderr)
                };

                if exit_code == 0 {
                    ToolExecutionResult::ok(output)
                } else {
                    ToolExecutionResult::err(format!("Exit code: {}\n{}", exit_code, output))
                }
            }
            Err(e) => ToolExecutionResult::err(format!("Command execution failed: {}", e)),
        }
    }

    async fn run_command(&self, command: &str, timeout_ms: u64) -> Result<(String, String, i32), String> {
        let child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn process: {}", e))?;

        let duration = Duration::from_millis(timeout_ms);
        
        match timeout(duration, child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);
                Ok((stdout, stderr, exit_code))
            }
            Ok(Err(e)) => Err(format!("Failed to wait for process: {}", e)),
            Err(_) => Err("Command timed out".to_string()),
        }
    }

    fn validate_command(&self, command: &str) -> Option<String> {
        let cmd_lower = command.to_lowercase();
        let trimmed = cmd_lower.trim();

        if let Some(ref allowed) = self.allowed_commands {
            let first_word = trimmed.split_whitespace().next().unwrap_or("");
            if !allowed.contains(first_word) {
                return Some(format!("Command not in whitelist: {}", first_word));
            }
            return None;
        }

        for dangerous in &self.dangerous_commands {
            if cmd_lower.contains(dangerous) {
                return Some(format!("Dangerous command: {}", dangerous));
            }
        }

        if trimmed.contains(":(){:|:&};:") {
            return Some("Fork bomb detected".to_string());
        }

        None
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dangerous_commands() {
        let tool = BashTool::new();
        assert!(tool.validate_command("rm -rf /").is_some());
        assert!(tool.validate_command(":(){:|:&};:").is_some());
    }

    #[test]
    fn test_safe_commands() {
        let tool = BashTool::new();
        assert!(tool.validate_command("ls -la").is_none());
        assert!(tool.validate_command("cat file.txt").is_none());
    }

    #[tokio::test]
    async fn test_execute_safe_command() {
        let tool = BashTool::new();
        let result = tool.execute("echo 'hello'", 5000).await;
        assert!(result.success);
    }
}
