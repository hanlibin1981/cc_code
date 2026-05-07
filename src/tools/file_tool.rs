//! 文件操作工具

use crate::tools::ToolExecutionResult;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncReadExt;

#[allow(dead_code)]
pub struct FileTool {
    max_file_size: usize,
}

impl FileTool {
    pub fn new() -> Self {
        Self {
            max_file_size: 10 * 1024 * 1024, // 10MB
        }
    }

    pub async fn read_file(&self, path: &str) -> ToolExecutionResult {
        let path = Path::new(path);
        if !self.is_safe_path(path) {
            return ToolExecutionResult::err("Path traversal detected");
        }

        // Check file size first
        match fs::metadata(path).await {
            Ok(meta) => {
                if meta.len() as usize > self.max_file_size {
                    return ToolExecutionResult::err(format!("File too large: {} bytes", meta.len()));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return ToolExecutionResult::err(format!("File not found: {:?}", path));
            }
            Err(e) => {
                return ToolExecutionResult::err(format!("Cannot read file metadata: {}", e));
            }
        }

        let mut file = match fs::File::open(path).await {
            Ok(f) => f,
            Err(e) => return ToolExecutionResult::err(format!("Cannot open file: {}", e)),
        };

        let mut contents = Vec::new();
        match file.read_to_end(&mut contents).await {
            Ok(_) => {}
            Err(e) => return ToolExecutionResult::err(format!("Cannot read file: {}", e)),
        }

        match String::from_utf8(contents) {
            Ok(s) => ToolExecutionResult::ok(s),
            Err(_) => ToolExecutionResult::err("File is not valid UTF-8".to_string()),
        }
    }

    pub async fn write_file(&self, path: &str, content: &str) -> ToolExecutionResult {
        let path = Path::new(path);
        if !self.is_safe_path(path) {
            return ToolExecutionResult::err("Path traversal detected".to_string());
        }

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = fs::create_dir_all(parent).await {
                    return ToolExecutionResult::err(format!("Cannot create parent directory: {}", e));
                }
            }
        }

        match fs::write(path, content).await {
            Ok(_) => ToolExecutionResult::ok(format!("File written: {:?}", path)),
            Err(e) => ToolExecutionResult::err(format!("Cannot write file: {}", e)),
        }
    }

    pub async fn list_directory(&self, path: &str) -> ToolExecutionResult {
        let path = Path::new(path);
        if !self.is_safe_path(path) {
            return ToolExecutionResult::err("Path traversal detected".to_string());
        }

        let mut entries = match fs::read_dir(path).await {
            Ok(e) => e,
            Err(e) => return ToolExecutionResult::err(format!("Cannot read directory: {}", e)),
        };

        let mut dir_entries = Vec::new();
        while let Some(entry) = entries.next_entry().await.unwrap_or(None) {
            let file_name = entry.file_name().to_string_lossy().to_string();
            let file_type = if entry.file_type().await.map(|ft| ft.is_dir()).unwrap_or(false) {
                "DIR"
            } else {
                "FILE"
            };
            let size = entry.metadata().await.map(|m| m.len()).unwrap_or(0);
            dir_entries.push(format!("{} {:>10} {}", file_type, size, file_name));
        }

        dir_entries.sort();
        ToolExecutionResult::ok(dir_entries.join("\n"))
    }

    pub async fn create_directory(&self, path: &str) -> ToolExecutionResult {
        let path = Path::new(path);
        if !self.is_safe_path(path) {
            return ToolExecutionResult::err("Path traversal detected".to_string());
        }

        match fs::create_dir_all(path).await {
            Ok(_) => ToolExecutionResult::ok(format!("Directory created: {:?}", path)),
            Err(e) => ToolExecutionResult::err(format!("Cannot create directory: {}", e)),
        }
    }

    pub async fn file_exists(&self, path: &str) -> ToolExecutionResult {
        let path = Path::new(path);
        if !self.is_safe_path(path) {
            return ToolExecutionResult::err("Path traversal detected".to_string());
        }
        ToolExecutionResult::ok(format!("{}", path.exists()))
    }

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

impl Default for FileTool {
    fn default() -> Self {
        Self::new()
    }
}
