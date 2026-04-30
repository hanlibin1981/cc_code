//! 工作区感知模块

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub root: PathBuf,
    pub language: Option<String>,
    pub project_type: Option<String>,
    pub has_git: bool,
    pub files: Vec<FileInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: PathBuf,
    pub file_type: FileType,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileType {
    Source(String),
    Config,
    Test,
    Documentation,
    Other,
}

impl FileType {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => FileType::Source("rust".to_string()),
            "py" => FileType::Source("python".to_string()),
            "js" => FileType::Source("javascript".to_string()),
            "ts" => FileType::Source("typescript".to_string()),
            "go" => FileType::Source("go".to_string()),
            "cpp" | "cc" | "cxx" => FileType::Source("cpp".to_string()),
            "c" => FileType::Source("c".to_string()),
            "java" => FileType::Source("java".to_string()),
            "rb" => FileType::Source("ruby".to_string()),
            "toml" | "yaml" | "yml" | "json" => FileType::Config,
            "md" | "rst" | "txt" => FileType::Documentation,
            "test.rs" | "_test.go" => FileType::Test,
            _ => FileType::Other,
        }
    }
}

pub struct WorkspaceDetector {
    workspace_root: PathBuf,
}

impl WorkspaceDetector {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    pub fn detect(&self) -> Result<WorkspaceInfo, String> {
        let mut files = Vec::new();
        self.scan_directory(&self.workspace_root, &mut files, 0, 3)?;

        let project_type = self.detect_project_type();
        let language = self.detect_language();
        let has_git = self.workspace_root.join(".git").exists();

        Ok(WorkspaceInfo {
            root: self.workspace_root.clone(),
            language,
            project_type,
            has_git,
            files,
        })
    }

    fn scan_directory(&self, dir: &Path, files: &mut Vec<FileInfo>, depth: usize, max_depth: usize) -> Result<(), String> {
        if depth > max_depth {
            return Ok(());
        }

        let entries = std::fs::read_dir(dir)
            .map_err(|e| format!("Cannot read directory {:?}: {}", dir, e))?;

        for entry in entries.flatten() {
            let path = entry.path();
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            if metadata.is_dir() {
                let name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                
                // Skip hidden and common ignored directories
                if name.starts_with('.') || name == "target" || name == "node_modules" || name == "__pycache__" {
                    continue;
                }
                
                self.scan_directory(&path, files, depth + 1, max_depth)?;
            } else {
                let file_name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                
                let extension = path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                
                files.push(FileInfo {
                    path: path.clone(),
                    file_type: FileType::from_extension(extension),
                    size: metadata.len(),
                });
            }
        }

        Ok(())
    }

    fn detect_project_type(&self) -> Option<String> {
        let indicators = [
            ("Cargo.toml", "rust"),
            ("package.json", "node"),
            ("go.mod", "go"),
            ("requirements.txt", "python"),
            ("Pipfile", "python"),
            ("pyproject.toml", "python"),
            ("pom.xml", "java"),
            ("build.gradle", "java"),
            ("CMakeLists.txt", "cpp"),
        ];

        for (file, project_type) in &indicators {
            if self.workspace_root.join(file).exists() {
                return Some(project_type.to_string());
            }
        }

        None
    }

    fn detect_language(&self) -> Option<String> {
        let mut counts: HashMap<String, usize> = HashMap::new();

        for entry in std::fs::read_dir(&self.workspace_root).into_iter().flatten().flatten() {
            if entry.path().is_file() {
                let ext = entry.path().extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                
                if !ext.is_empty() {
                    *counts.entry(ext).or_insert(0) += 1;
                }
            }
        }

        counts.into_iter().max_by_key(|(_, count)| *count).map(|(ext, _)| ext)
    }

    pub fn get_workspace_files(&self, extensions: Option<Vec<&str>>) -> Result<Vec<PathBuf>, String> {
        let mut result = Vec::new();
        
        for entry in self.scan_files()? {
            if let Some(ref exts) = extensions {
                let ext = entry.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                if exts.contains(&ext) {
                    result.push(entry);
                }
            } else {
                result.push(entry);
            }
        }

        Ok(result)
    }

    fn scan_files(&self) -> Result<Vec<PathBuf>, String> {
        let mut files = Vec::new();
        self.scan_directory(&self.workspace_root, &mut files, 0, 3)?;
        Ok(files.into_iter().map(|f| f.path).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_workspace_detection() {
        let current_dir = env::current_dir().unwrap();
        let detector = WorkspaceDetector::new(current_dir.clone());
        let info = detector.detect().unwrap();
        
        assert_eq!(info.root, current_dir);
    }

    #[test]
    fn test_file_type_detection() {
        assert!(matches!(FileType::from_extension("rs"), FileType::Source(s) if s == "rust"));
        assert!(matches!(FileType::from_extension("py"), FileType::Source(s) if s == "python"));
        assert!(matches!(FileType::from_extension("toml"), FileType::Config));
    }
}
