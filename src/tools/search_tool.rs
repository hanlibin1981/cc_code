//! 文件搜索工具 - glob + grep
//! 
//! 特性：
//! - glob: 按模式搜索文件（支持 **, *, ?）
//! - grep: 在文件中搜索内容（支持正则）
//! - 尊重 .gitignore 和常见忽略规则

use crate::tools::ToolExecutionResult;
use std::path::{Path, PathBuf};
use std::collections::HashSet;
use tokio::fs;
use walkdir::{WalkDir, WalkDirIterator};

/// 最大搜索结果数
const MAX_RESULTS: usize = 500;
/// 最大文件扫描大小（字节）
const MAX_SCAN_SIZE: usize = 1024 * 1024; // 1MB

pub struct SearchTool {
    /// 忽略的目录
    ignored_dirs: HashSet<&'static str>,
    /// 忽略的文件
    ignored_files: HashSet<&'static str>,
}

impl SearchTool {
    pub fn new() -> Self {
        let mut ignored_dirs = HashSet::new();
        ignored_dirs.insert("target");
        ignored_dirs.insert("node_modules");
        ignored_dirs.insert(".git");
        ignored_dirs.insert(".svn");
        ignored_dirs.insert("__pycache__");
        ignored_dirs.insert(".pytest_cache");
        ignored_dirs.insert("dist");
        ignored_dirs.insert("build");
        ignored_dirs.insert(".idea");
        ignored_dirs.insert(".vscode");

        let mut ignored_files = HashSet::new();
        ignored_files.insert(".DS_Store");
        ignored_files.insert("Thumbs.db");

        Self {
            ignored_dirs,
            ignored_files,
        }
    }

    /// glob 模式搜索文件
    pub async fn glob(&self, pattern: &str, base_dir: Option<&str>) -> ToolExecutionResult {
        let base = match base_dir {
            Some(d) => PathBuf::from(d),
            None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        };

        if !base.exists() {
            return ToolExecutionResult::err(format!("Directory does not exist: {:?}", base));
        }

        let pattern = pattern.trim();
        if pattern.is_empty() {
            return ToolExecutionResult::err("Empty glob pattern");
        }

        let mut results = Vec::new();
        let walker = WalkDir::new(&base)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !self.is_ignored_entry(e));

        for entry in walker {
            if results.len() >= MAX_RESULTS {
                break;
            }

            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let relative_path = match entry.path().strip_prefix(&base) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let path_str = relative_path.to_string_lossy();

            // 简单 glob 匹配
            if self.match_glob(&path_str, pattern) {
                let metadata = entry.metadata().ok();
                let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                results.push(format!("{:>10} {}", size, path_str));
            }
        }

        results.sort();
        
        if results.is_empty() {
            ToolExecutionResult::ok(format!("No files matching pattern: {}", pattern))
        } else {
            let header = format!("Found {} file(s):\n", results.len());
            ToolExecutionResult::ok(header + &results.join("\n"))
        }
    }

    /// grep 在文件中搜索内容
    pub async fn grep(
        &self,
        pattern: &str,
        paths: Option<&[String]>,
        base_dir: Option<&str>,
        case_sensitive: bool,
    ) -> ToolExecutionResult {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return ToolExecutionResult::err("Empty search pattern");
        }

        let regex = if case_sensitive {
            regex::Regex::new(pattern)
        } else {
            regex::Regex::new(&pattern.to_lowercase())
        };

        let regex = match regex {
            Ok(r) => r,
            Err(e) => return ToolExecutionResult::err(format!("Invalid regex: {}", e)),
        };

        let base = match base_dir {
            Some(d) => PathBuf::from(d),
            None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        };

        let paths_to_search: Vec<PathBuf> = if let Some(p) = paths {
            p.iter().map(|s| base.join(s)).collect()
        } else {
            vec![base.clone()]
        };

        let mut matches = Vec::new();
        let mut searched_files = 0usize;

        for path in paths_to_search {
            if matches.len() >= MAX_RESULTS {
                break;
            }

            if path.is_file() {
                searched_files += 1;
                let file_matches = self.search_file(&path, &regex, case_sensitive).await;
                matches.extend(file_matches);
            } else if path.is_dir() {
                let walker = WalkDir::new(&path)
                    .follow_links(false)
                    .into_iter()
                    .filter_entry(|e| !self.is_ignored_entry(e));

                for entry in walker {
                    if matches.len() >= MAX_RESULTS {
                        break;
                    }

                    let entry = match entry {
                        Ok(e) => e,
                        Err(_) => continue,
                    };

                    if !entry.file_type().is_file() {
                        continue;
                    }

                    searched_files += 1;
                    let file_path = entry.path().to_path_buf();
                    let file_matches = self.search_file(&file_path, &regex, case_sensitive).await;
                    
                    for m in file_matches {
                        let relative = file_path.strip_prefix(&base).unwrap_or(&file_path);
                        matches.push(format!("{}:{}", relative.display(), m));
                    }
                }
            }
        }

        if matches.is_empty() {
            ToolExecutionResult::ok(format!(
                "No matches found for pattern: {} (searched {} files)",
                pattern, searched_files
            ))
        } else {
            let header = format!(
                "Found {} match(es) in {} file(s):\n",
                matches.len(),
                searched_files
            );
            ToolExecutionResult::ok(header + &matches.join("\n"))
        }
    }

    /// 在单个文件中搜索
    async fn search_file(&self, path: &Path, regex: &regex::Regex, case_insensitive: bool) -> Vec<String> {
        let mut results = Vec::new();

        if let Ok(metadata) = fs::metadata(path).await {
            if metadata.len() > MAX_SCAN_SIZE as u64 {
                return results;
            }
        }

        let content = match fs::read_to_string(path).await {
            Ok(c) => c,
            Err(_) => return results,
        };

        if content.contains('\0') {
            return results;
        }

        let search_text = if case_insensitive {
            content.to_lowercase()
        } else {
            content.clone()
        };

        for (line_num, line) in search_text.lines().enumerate() {
            if regex.is_match(line) {
                let original_line = content.lines().nth(line_num).unwrap_or("");
                if original_line.len() > 200 {
                    results.push(format!("{:>4}: {}", line_num + 1, truncate(original_line, 200)));
                } else {
                    results.push(format!("{:>4}: {}", line_num + 1, original_line));
                }

                if results.len() >= 100 {
                    break;
                }
            }
        }

        results
    }

    /// 检查是否应该忽略该条目
    fn is_ignored_entry(&self, entry: &walkdir::DirEntry) -> bool {
        let name = entry.file_name().to_string_lossy();
        
        if name.starts_with('.') {
            return true;
        }

        if entry.file_type().is_dir() {
            return self.ignored_dirs.contains(name.as_ref());
        }

        if entry.file_type().is_file() {
            return self.ignored_files.contains(name.as_ref());
        }

        false
    }

    /// glob pattern segment -> regex (single path component, no slashes)
    fn glob_seg_to_regex(pattern: &str) -> String {
        let mut result = String::from("^");
        for c in pattern.chars() {
            match c {
                '*' => result.push_str("[^\"/]*"),
                '?' => result.push_str("[^\"/]"),
                '\\' | '.' | '+' | '^' | '$' | '(' | ')' | '|' | '[' | ']' | '{' | '}' => {
                    result.push('\\');
                    result.push(c);
                }
                c => result.push(c),
            }
        }
        result
    }

    fn match_glob(&self, path: &str, pattern: &str) -> bool {
        // 处理 ** 递归匹配：转换为正则
        if pattern.contains("**") {
            let regex_pattern = Self::glob_pattern_to_regex(pattern);
            let re = regex::Regex::new(&regex_pattern);
            return re.map(|r| r.is_match(path)).unwrap_or(false);
        }

        // 对齐尾部匹配
        let pattern_parts: Vec<&str> = pattern.split(['/', '\\']).collect();
        let path_parts: Vec<&str> = path.split(['/', '\\']).collect();

        if pattern_parts.len() > path_parts.len() {
            return false;
        }

        let start_offset = path_parts.len() - pattern_parts.len();

        for (i, pattern_part) in pattern_parts.iter().enumerate() {
            let path_part = path_parts[start_offset + i];
            if !Self::match_part_glob(path_part, pattern_part) {
                return false;
            }
        }

        true
    }

    fn match_part_glob(name: &str, pattern: &str) -> bool {
        let regex_pattern = Self::glob_seg_to_regex(pattern);
        regex::Regex::new(&regex_pattern)
            .map(|r| r.is_match(name))
            .unwrap_or(false)
    }

    /// 将包含 ** 的 glob pattern 转换为正则表达式
    fn glob_pattern_to_regex(pattern: &str) -> String {
        let mut result = String::from("^");
        let chars: Vec<char> = pattern.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
                // ** 匹配任意路径段（可包含斜杠）
                if i + 2 < chars.len() && chars[i + 2] == '/' {
                    // **/ -> 匹配 0 或多个子目录
                    result.push_str("(?:.*/)?");
                    i += 3;
                } else {
                    // ** 在末尾 -> 匹配任意内容
                    result.push_str("(?:.*/)?");
                    i += 2;
                }
            } else if chars[i] == '*' {
                result.push_str("[^\"/]*");
                i += 1;
            } else if chars[i] == '?' {
                result.push_str("[^\"/]");
                i += 1;
            } else if chars[i] == '\\' || chars[i] == '.' || chars[i] == '+'
                || chars[i] == '^' || chars[i] == '$' || chars[i] == '('
                || chars[i] == ')' || chars[i] == '|' || chars[i] == '['
                || chars[i] == ']' || chars[i] == '{' || chars[i] == '}' || chars[i] == '*'
            {
                result.push('\\');
                result.push(chars[i]);
                i += 1;
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }
        result.push('$');
        result
    }

    /// find 命令实现
    pub async fn find(&self, path: &str, name_pattern: Option<&str>, file_type: Option<&str>) -> ToolExecutionResult {
        let base = PathBuf::from(path);
        
        if !base.exists() {
            return ToolExecutionResult::err(format!("Path does not exist: {}", path));
        }

        let mut results = Vec::new();

        let type_filter = match file_type {
            Some(t) => match t {
                "f" | "file" => FileTypeFilter::File,
                "d" | "dir" | "directory" => FileTypeFilter::Directory,
                _ => FileTypeFilter::Any,
            },
            None => FileTypeFilter::Any,
        };

        let name_regex = name_pattern.map(|p| {
            let mut regex_pattern = String::from("^");
            for c in p.chars() {
                match c {
                    '*' => regex_pattern.push_str(".*"),
                    '?' => regex_pattern.push('.'),
                    '.' => regex_pattern.push_str("\\."),
                    c if c.is_alphanumeric() || c == '_' || c == '-' => regex_pattern.push(c),
                    c => regex_pattern.push_str(&regex::escape(&c.to_string())),
                }
            }
            regex_pattern.push('$');
            regex::Regex::new(&regex_pattern).ok()
        }).flatten();

        let walker = WalkDir::new(&base)
            .follow_links(false)
            .max_depth(if name_pattern.is_some() { usize::MAX } else { 3 })
            .into_iter()
            .filter_entry(|e| !self.is_ignored_entry(e));

        for entry in walker {
            if results.len() >= MAX_RESULTS {
                break;
            }

            let entry: walkdir::DirEntry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            match type_filter {
                FileTypeFilter::File if !entry.file_type().is_file() => continue,
                FileTypeFilter::Directory if !entry.file_type().is_dir() => continue,
                FileTypeFilter::Any | FileTypeFilter::File | FileTypeFilter::Directory => {}
            }

            if let Some(ref regex) = name_regex {
                let name = entry.file_name().to_string_lossy();
                if !regex.is_match(&name) {
                    continue;
                }
            }

            let relative = entry.path().strip_prefix(&base).unwrap_or(entry.path());
            results.push(relative.display().to_string());
        }

        results.sort();
        
        if results.is_empty() {
            ToolExecutionResult::ok("No matches found".to_string())
        } else {
            ToolExecutionResult::ok(results.join("\n"))
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum FileTypeFilter {
    Any,
    File,
    Directory,
}

impl Default for SearchTool {
    fn default() -> Self {
        Self::new()
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...[truncated]", &s[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_matching() {
        let tool = SearchTool::new();
        assert!(tool.match_glob("src/main.rs", "*.rs"));
        assert!(tool.match_glob("src/main.rs", "src/*.rs"));
        assert!(tool.match_glob("src/lib.rs", "src/*.rs"));
        assert!(!tool.match_glob("tests/main.rs", "src/*.rs"));
        assert!(tool.match_glob("src/foo/main.rs", "src/**/*.rs"));
    }

    #[test]
    fn test_find_name_pattern() {
        let tool = SearchTool::new();
        assert!(tool.match_glob("main.rs", "main.*"));
        assert!(tool.match_glob("test_main.py", "*_main.py"));
        assert!(!tool.match_glob("foo_main.rs", "main.*"));
    }
}
