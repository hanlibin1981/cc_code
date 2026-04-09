//! 任务拆解模块
//! 将复杂编程任务拆解为可执行的步骤

use serde::{Deserialize, Serialize};

/// 任务步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStep {
    pub id: usize,
    pub description: String,
    pub tool: Option<String>,
    pub status: StepStatus,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Skipped,
}

/// 任务规划器
#[derive(Debug, Clone)]
pub struct TaskPlanner;

impl TaskPlanner {
    /// 解析用户输入，生成任务步骤
    pub fn plan(task_description: &str) -> Vec<TaskStep> {
        // 简单实现：基于关键词拆解
        let mut steps = Vec::new();
        let mut step_id = 1;

        let desc_lower = task_description.to_lowercase();

        // 检测常见任务类型
        if desc_lower.contains("创建") || desc_lower.contains("新建") {
            if desc_lower.contains("项目") || desc_lower.contains("应用") {
                steps.push(TaskStep {
                    id: step_id,
                    description: "创建项目目录结构".into(),
                    tool: Some("bash".into()),
                    status: StepStatus::Pending,
                    notes: Some("mkdir -p 创建目录".into()),
                });
                step_id += 1;
            }

            if desc_lower.contains("文件") {
                steps.push(TaskStep {
                    id: step_id,
                    description: "创建必要的文件".into(),
                    tool: Some("write_file".into()),
                    status: StepStatus::Pending,
                    notes: None,
                });
                step_id += 1;
            }
        }

        if desc_lower.contains("修改") || desc_lower.contains("更新") {
            steps.push(TaskStep {
                id: step_id,
                description: "分析现有代码结构".into(),
                tool: Some("glob".into()),
                status: StepStatus::Pending,
                notes: None,
            });
            step_id += 1;

            steps.push(TaskStep {
                id: step_id,
                description: "定位需要修改的文件".into(),
                tool: Some("grep".into()),
                status: StepStatus::Pending,
                notes: None,
            });
            step_id += 1;

            steps.push(TaskStep {
                id: step_id,
                description: "执行修改".into(),
                tool: Some("edit_file".into()),
                status: StepStatus::Pending,
                notes: None,
            });
            step_id += 1;
        }

        if desc_lower.contains("测试") || desc_lower.contains("test") {
            steps.push(TaskStep {
                id: step_id,
                description: "运行测试验证功能".into(),
                tool: Some("bash".into()),
                status: StepStatus::Pending,
                notes: Some("使用合适的测试命令".into()),
            });
            step_id += 1;
        }

        if desc_lower.contains("调试") || desc_lower.contains("debug") {
            steps.push(TaskStep {
                id: step_id,
                description: "复现问题".into(),
                tool: Some("bash".into()),
                status: StepStatus::Pending,
                notes: None,
            });
            step_id += 1;

            steps.push(TaskStep {
                id: step_id,
                description: "分析问题原因".into(),
                tool: Some("grep".into()),
                status: StepStatus::Pending,
                notes: None,
            });
            step_id += 1;
        }

        if steps.is_empty() {
            // 默认步骤
            steps.push(TaskStep {
                id: step_id,
                description: "理解任务需求".into(),
                tool: None,
                status: StepStatus::Pending,
                notes: None,
            });
            step_id += 1;

            steps.push(TaskStep {
                id: step_id,
                description: "执行编程任务".into(),
                tool: None,
                status: StepStatus::Pending,
                notes: None,
            });
            step_id += 1;

            steps.push(TaskStep {
                id: step_id,
                description: "验证结果".into(),
                tool: Some("bash".into()),
                status: StepStatus::Pending,
                notes: Some("运行测试或构建验证".into()),
            });
        }

        steps
    }

    /// 从模型输出中提取下一步计划
    pub fn parse_plan_from_response(response: &str) -> Option<String> {
        // 查找 "下一步" 或 "next step" 等关键词
        let lines: Vec<&str> = response.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            let lower = line.to_lowercase();
            if lower.contains("下一步") || lower.contains("next step") {
                // 返回下一行内容
                if i + 1 < lines.len() {
                    return Some(lines[i + 1].trim().to_string());
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_create() {
        let steps = TaskPlanner::plan("创建一个新的 Rust 项目");
        assert!(!steps.is_empty());
        assert!(steps[0].tool.is_some());
    }

    #[test]
    fn test_plan_update() {
        let steps = TaskPlanner::plan("修改 src/main.rs 中的函数");
        assert!(steps.len() >= 3);
    }
}
