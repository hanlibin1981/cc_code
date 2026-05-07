//! 文件编辑工具

use crate::tools::ToolExecutionResult;
use std::path::Path;
use tokio::fs;

pub struct EditTool {
    max_file_size: usize,
}

impl EditTool {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            max_file_size: 5 * 1024 * 1024, // 5MB
        }
    }

    #[allow(dead_code)]
    pub async fn edit_file(&self, path: &str, old_text: &str, new_text: &str) -> ToolExecutionResult {
        let path = Path::new(path);
        if !self.is_safe_path(path) {
            return ToolExecutionResult::err("Path traversal detected");
        }

        let content = match fs::read_to_string(path).await {
            Ok(c) => c,
            Err(e) => return ToolExecutionResult::err(format!("Cannot read file: {}", e)),
        };

        if content.len() > self.max_file_size {
            return ToolExecutionResult::err("File too large to edit");
        }

        if !content.contains(old_text) {
            return ToolExecutionResult::err(format!(
                "Text not found in file.\nExpected:\n{}\n\nFile content:\n{}",
                truncate(old_text, 200),
                truncate(&content, 500)
            ));
        }

        let new_content = content.replace(old_text, new_text);

        match fs::write(path, &new_content).await {
            Ok(_) => ToolExecutionResult::ok(format!(
                "File edited successfully.\nChanges:\n- Replaced {} bytes with {} bytes",
                old_text.len(),
                new_text.len()
            )),
            Err(e) => ToolExecutionResult::err(format!("Cannot write file: {}", e)),
        }
    }

    #[allow(dead_code)]
    pub async fn append_to_file(&self, path: &str, content: &str) -> ToolExecutionResult {
        let path = Path::new(path);
        if !self.is_safe_path(path) {
            return ToolExecutionResult::err("Path traversal detected");
        }

        let current = fs::read_to_string(path).await.unwrap_or_default();
        let new_content = format!("{}{}", current, content);

        match fs::write(path, &new_content).await {
            Ok(_) => ToolExecutionResult::ok("Content appended successfully".to_string()),
            Err(e) => ToolExecutionResult::err(format!("Cannot append to file: {}", e)),
        }
    }

    #[allow(dead_code)]
    fn is_safe_path(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        if path_str.contains("..") {
            return false;
        }
        let forbidden = ["/etc", "/sys", "/proc", "/dev"];
        for f in &forbidden {
            if path_str.starts_with(f) {
                return false;
            }
        }
        true
    }
}

impl Default for EditTool {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
fn truncate(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}...[truncated {} chars]", &text[..max_len], text.len() - max_len)
    }
}
