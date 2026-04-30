//! Coordinator 多Agent编排
//! 参考 Claude Code 的 coordinatorMode.ts
//! 
//! 特性：
//! - 研究阶段：worker 并行探索（只读）
//! - 综合阶段：coordinator 汇总发现
//! - 实现阶段：worker 按 spec 执行
//! - 验证阶段：独立 worker 验证

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Coordinator 阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinatorPhase {
    /// 研究阶段
    Research,
    /// 综合阶段
    Synthesis,
    /// 实现阶段
    Implementation,
    /// 验证阶段
    Verification,
    /// 完成
    Completed,
    /// 失败
    Failed,
}

impl std::fmt::Display for CoordinatorPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoordinatorPhase::Research => write!(f, "Research"),
            CoordinatorPhase::Synthesis => write!(f, "Synthesis"),
            CoordinatorPhase::Implementation => write!(f, "Implementation"),
            CoordinatorPhase::Verification => write!(f, "Verification"),
            CoordinatorPhase::Completed => write!(f, "Completed"),
            CoordinatorPhase::Failed => write!(f, "Failed"),
        }
    }
}

/// Worker 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerType {
    /// 研究 worker（只读）
    Research,
    /// 实现 worker（可写）
    Implementation,
    /// 验证 worker（只读）
    Verification,
}

/// Worker 信息
#[derive(Debug, Clone)]
pub struct Worker {
    /// Worker ID
    pub id: String,
    /// 类型
    pub worker_type: WorkerType,
    /// 状态
    pub state: WorkerState,
    /// 工作目录
    pub working_directory: String,
    /// 当前任务
    pub current_task: Option<String>,
    /// 完成的任务
    pub completed_tasks: Vec<String>,
    /// 发现结果
    pub findings: Vec<Finding>,
    /// 错误信息
    pub error: Option<String>,
}

/// Worker 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerState {
    /// 空闲
    Idle,
    /// 运行中
    Running,
    /// 等待
    Waiting,
    /// 完成
    Completed,
    /// 失败
    Failed,
}

/// 发现结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// 发现 ID
    pub id: String,
    /// 类型
    pub finding_type: FindingType,
    /// 描述
    pub description: String,
    /// 证据/详情
    pub evidence: Vec<String>,
    /// 置信度（0-1）
    pub confidence: f32,
    /// 来源
    pub source: String,
}

/// 发现类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingType {
    /// 信息
    Information,
    /// 架构
    Architecture,
    /// 问题
    Issue,
    /// 建议
    Suggestion,
    /// 风险
    Risk,
}

/// 实现规范
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplementationSpec {
    /// 规范 ID
    pub id: String,
    /// 描述
    pub description: String,
    /// 优先级
    pub priority: u32,
    /// 依赖
    pub dependencies: Vec<String>,
    /// 状态
    pub status: SpecStatus,
}

/// 规范状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecStatus {
    /// 待实现
    Pending,
    /// 进行中
    InProgress,
    /// 已完成
    Completed,
    /// 失败
    Failed,
    /// 跳过
    Skipped,
}

/// Coordinator 配置
#[derive(Debug, Clone)]
pub struct CoordinatorConfig {
    /// 最大并行 worker 数
    pub max_parallel_workers: usize,
    /// 研究阶段超时（秒）
    pub research_timeout_secs: u64,
    /// 实现阶段超时（秒）
    pub implementation_timeout_secs: u64,
    /// 验证阶段超时（秒）
    pub verification_timeout_secs: u64,
    /// 最小研究worker数
    pub min_research_workers: usize,
    /// 最大研究worker数
    pub max_research_workers: usize,
    /// 是否启用验证阶段
    pub enable_verification: bool,
    /// 验证通过阈值（0-1）
    pub verification_threshold: f32,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            max_parallel_workers: 5,
            research_timeout_secs: 300,
            implementation_timeout_secs: 600,
            verification_timeout_secs: 180,
            min_research_workers: 1,
            max_research_workers: 3,
            enable_verification: true,
            verification_threshold: 0.8,
        }
    }
}

/// Coordinator 状态
#[derive(Debug, Clone)]
pub struct CoordinatorState {
    /// 当前阶段
    pub phase: CoordinatorPhase,
    /// 总体进度（0-100）
    pub progress: u32,
    /// 研究发现
    pub research_findings: Vec<Finding>,
    /// 实现规范
    pub specs: Vec<ImplementationSpec>,
    /// Worker 列表
    pub workers: Vec<Worker>,
    /// 阶段开始时间
    pub phase_start_time: Option<chrono::DateTime<chrono::Utc>>,
    /// 最终报告
    pub final_report: Option<String>,
    /// 错误信息
    pub error: Option<String>,
}

impl Default for CoordinatorState {
    fn default() -> Self {
        Self {
            phase: CoordinatorPhase::Research,
            progress: 0,
            research_findings: Vec::new(),
            specs: Vec::new(),
            workers: Vec::new(),
            phase_start_time: None,
            final_report: None,
            error: None,
        }
    }
}

/// Coordinator
pub struct Coordinator {
    config: CoordinatorConfig,
    state: Arc<RwLock<CoordinatorState>>,
    /// 用户提示
    original_prompt: String,
}

impl Default for Coordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl Coordinator {
    pub fn new() -> Self {
        Self {
            config: CoordinatorConfig::default(),
            state: Arc::new(RwLock::new(CoordinatorState::default())),
            original_prompt: String::new(),
        }
    }

    /// 配置
    pub fn with_config(mut self, config: CoordinatorConfig) -> Self {
        self.config = config;
        self
    }

    /// 设置原始提示
    pub fn set_prompt(&mut self, prompt: String) {
        self.original_prompt = prompt;
    }

    /// 初始化
    pub async fn initialize(&self) -> Result<(), String> {
        let mut state = self.state.write().await;
        state.phase = CoordinatorPhase::Research;
        state.progress = 0;
        state.phase_start_time = Some(chrono::Utc::now());
        Ok(())
    }

    // ==================== 研究阶段 ====================

    /// 启动研究阶段
    pub async fn start_research(&self, prompt: &str) -> Result<Vec<String>, String> {
        let mut state = self.state.write().await;
        state.phase = CoordinatorPhase::Research;
        state.phase_start_time = Some(chrono::Utc::now());
        drop(state);

        // 创建研究 worker
        let worker_count = self.config.max_research_workers.min(self.config.max_parallel_workers);
        let mut worker_ids = Vec::new();

        for i in 0..worker_count {
            let worker_id = format!("research_worker_{}", i);
            let worker = Worker {
                id: worker_id.clone(),
                worker_type: WorkerType::Research,
                state: WorkerState::Running,
                working_directory: self.get_worker_directory(&worker_id),
                current_task: Some(prompt.to_string()),
                completed_tasks: Vec::new(),
                findings: Vec::new(),
                error: None,
            };

            let mut state = self.state.write().await;
            state.workers.push(worker);
            worker_ids.push(worker_id);
        }

        Ok(worker_ids)
    }

    /// Worker 发现结果
    pub async fn add_research_finding(&self, worker_id: &str, finding: Finding) {
        let mut state = self.state.write().await;

        // 添加到研究结果
        state.research_findings.push(finding.clone());

        // 更新 worker
        if let Some(worker) = state.workers.iter_mut().find(|w| w.id == worker_id) {
            worker.findings.push(finding.clone());
        }
    }

    /// 完成研究阶段
    pub async fn complete_research(&self) -> Result<Vec<Finding>, String> {
        let mut state = self.state.write().await;
        state.progress = 25;

        // 更新所有研究 worker 状态
        for worker in &mut state.workers {
            if worker.worker_type == WorkerType::Research {
                worker.state = WorkerState::Completed;
                worker.current_task = None;
            }
        }

        // 合成规范
        let specs = self.synthesize_specs_internal(&state.research_findings);
        state.specs = specs;
        state.phase = CoordinatorPhase::Synthesis;
        state.phase_start_time = Some(chrono::Utc::now());

        Ok(state.research_findings.clone())
    }

    // ==================== 综合阶段 ====================

    /// 执行综合
    pub async fn synthesize(&self) -> Result<Vec<ImplementationSpec>, String> {
        let state = self.state.read().await;

        if state.phase != CoordinatorPhase::Synthesis {
            return Err("Not in synthesis phase".to_string());
        }

        // 更新进度
        let mut state = self.state.write().await;
        state.progress = 35;

        Ok(state.specs.clone())
    }

    /// 完成综合
    pub async fn complete_synthesis(&self) {
        let mut state = self.state.write().await;
        state.progress = 40;
        state.phase = CoordinatorPhase::Implementation;
        state.phase_start_time = Some(chrono::Utc::now());
    }

    /// 内部：合成规范
    fn synthesize_specs_internal(&self, findings: &[Finding]) -> Vec<ImplementationSpec> {
        let mut specs: Vec<ImplementationSpec> = Vec::new();
        let mut spec_id = 0;

        for finding in findings {
            if matches!(finding.finding_type, FindingType::Issue | FindingType::Risk | FindingType::Suggestion) {
                specs.push(ImplementationSpec {
                    id: format!("spec_{}", spec_id),
                    description: finding.description.clone(),
                    priority: if matches!(finding.finding_type, FindingType::Risk) { 1 }
                              else if matches!(finding.finding_type, FindingType::Issue) { 2 }
                              else { 3 },
                    dependencies: Vec::new(),
                    status: SpecStatus::Pending,
                });
                spec_id += 1;
            }
        }

        // 按优先级排序
        specs.sort_by_key(|s| s.priority);
        specs
    }

    // ==================== 实现阶段 ====================

    /// 启动实现阶段
    pub async fn start_implementation(&self) -> Result<Vec<String>, String> {
        let state = self.state.read().await;

        if state.phase != CoordinatorPhase::Implementation {
            return Err("Not in implementation phase".to_string());
        }

        drop(state);

        // 创建实现 worker
        let worker_count = self.config.max_parallel_workers.min(3);
        let mut worker_ids = Vec::new();

        for i in 0..worker_count {
            let worker_id = format!("impl_worker_{}", i);
            let worker = Worker {
                id: worker_id.clone(),
                worker_type: WorkerType::Implementation,
                state: WorkerState::Running,
                working_directory: self.get_worker_directory(&worker_id),
                current_task: None,
                completed_tasks: Vec::new(),
                findings: Vec::new(),
                error: None,
            };

            let mut state = self.state.write().await;
            state.workers.push(worker);
            worker_ids.push(worker_id);
        }

        Ok(worker_ids)
    }

    /// 分配实现任务
    pub async fn assign_implementation_task(&self, worker_id: &str, spec_id: &str) {
        let mut state = self.state.write().await;

        // 先找到 spec
        let spec_desc = state.specs.iter_mut()
            .find(|s| s.id == spec_id)
            .map(|s| {
                s.status = SpecStatus::InProgress;
                s.description.clone()
            });

        // 再更新 worker
        if let Some(desc) = spec_desc {
            if let Some(worker) = state.workers.iter_mut().find(|w| w.id == worker_id) {
                worker.current_task = Some(desc);
            }
        }
    }

    /// 完成实现任务
    pub async fn complete_implementation_task(
        &self,
        worker_id: &str,
        spec_id: &str,
        success: bool,
    ) {
        let mut state = self.state.write().await;

        if let Some(worker) = state.workers.iter_mut().find(|w| w.id == worker_id) {
            if let Some(task) = worker.current_task.take() {
                worker.completed_tasks.push(task);
            }
            worker.state = if success { WorkerState::Idle } else { WorkerState::Failed };
        }

        if let Some(spec) = state.specs.iter_mut().find(|s| s.id == spec_id) {
            spec.status = if success { SpecStatus::Completed } else { SpecStatus::Failed };
        }

        // 更新进度
        let completed = state.specs.iter().filter(|s| s.status == SpecStatus::Completed).count();
        let total = state.specs.len();
        if total > 0 {
            state.progress = 40 + ((40 * completed / total) as u32);
        }
    }

    /// 完成实现阶段
    pub async fn complete_implementation(&self) -> Result<(), String> {
        let state = self.state.read().await;

        if state.phase != CoordinatorPhase::Implementation {
            return Err("Not in implementation phase".to_string());
        }

        // 检查是否有失败的规范
        let failed = state.specs.iter().any(|s| s.status == SpecStatus::Failed);
        if failed {
            return Err("Some specs failed to implement".to_string());
        }

        drop(state);

        if self.config.enable_verification {
            let mut state = self.state.write().await;
            state.phase = CoordinatorPhase::Verification;
            state.phase_start_time = Some(chrono::Utc::now());
        } else {
            let mut state = self.state.write().await;
            state.phase = CoordinatorPhase::Completed;
            state.progress = 100;
        }

        Ok(())
    }

    // ==================== 验证阶段 ====================

    /// 启动验证阶段
    pub async fn start_verification(&self) -> Result<Vec<String>, String> {
        let state = self.state.read().await;

        if state.phase != CoordinatorPhase::Verification {
            return Err("Not in verification phase".to_string());
        }

        drop(state);

        // 创建验证 worker
        let worker_id = "verification_worker_0".to_string();
        let worker = Worker {
            id: worker_id.clone(),
            worker_type: WorkerType::Verification,
            state: WorkerState::Running,
            working_directory: self.get_worker_directory(&worker_id),
            current_task: None,
            completed_tasks: Vec::new(),
            findings: Vec::new(),
            error: None,
        };

        let mut state = self.state.write().await;
        state.workers.push(worker);

        Ok(vec![worker_id])
    }

    /// 添加验证发现
    pub async fn add_verification_finding(&self, finding: Finding) {
        let mut state = self.state.write().await;

        if let Some(worker) = state.workers.iter_mut().find(|w| w.worker_type == WorkerType::Verification) {
            worker.findings.push(finding.clone());
        }
    }

    /// 完成验证
    pub async fn complete_verification(&self, passed: bool) -> Result<bool, String> {
        let mut state = self.state.write().await;
        state.progress = 95;

        if !passed && state.error.is_none() {
            state.error = Some("Verification failed".to_string());
            state.phase = CoordinatorPhase::Failed;
            return Ok(false);
        }

        state.phase = CoordinatorPhase::Completed;
        state.progress = 100;
        Ok(true)
    }

    // ==================== 辅助方法 ====================

    /// 获取 worker 目录
    fn get_worker_directory(&self, worker_id: &str) -> String {
        format!("/tmp/coordinator_{}", worker_id)
    }

    /// 获取当前状态
    pub async fn get_state(&self) -> CoordinatorState {
        self.state.read().await.clone()
    }

    /// 获取当前阶段
    pub async fn get_phase(&self) -> CoordinatorPhase {
        self.state.read().await.phase
    }

    /// 获取进度
    pub async fn get_progress(&self) -> u32 {
        self.state.read().await.progress
    }

    /// 检查是否完成
    pub async fn is_complete(&self) -> bool {
        let phase = self.get_phase().await;
        matches!(phase, CoordinatorPhase::Completed | CoordinatorPhase::Failed)
    }

    /// 生成最终报告
    pub async fn generate_report(&self) -> String {
        let state = self.state.read().await;

        let mut report = format!(
            "# Coordinator Report\n\n\
             ## Summary\n\
             - Phase: {}\n\
             - Progress: {}%\n\
             - Original Prompt: {}\n\n",
            state.phase, state.progress, self.original_prompt
        );

        // 研究发现
        report.push_str("## Research Findings\n\n");
        for finding in &state.research_findings {
            report.push_str(&format!(
                "- **[{:?}]** ({:.0}% confidence) {}\n  Evidence: {}\n",
                finding.finding_type,
                finding.confidence * 100.0,
                finding.description,
                finding.evidence.join(", ")
            ));
        }

        // 实现规范
        report.push_str("\n## Implementation Specs\n\n");
        for spec in &state.specs {
            report.push_str(&format!(
                "- **[{:?}]** Priority {}: {}\n",
                spec.status, spec.priority, spec.description
            ));
        }

        // 错误（如果有）
        if let Some(ref error) = state.error {
            report.push_str(&format!("\n## Error\n\n{}\n", error));
        }

        report
    }

    /// 重置
    pub async fn reset(&mut self) {
        let mut state = self.state.write().await;
        *state = CoordinatorState::default();
        self.original_prompt = String::new();
    }
}

/// Coordinator 请求（用于 MCP 协议）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorRequest {
    /// 请求类型
    pub request_type: CoordinatorRequestType,
    /// 用户提示
    pub prompt: Option<String>,
    /// Worker ID
    pub worker_id: Option<String>,
    /// 规范 ID
    pub spec_id: Option<String>,
    /// 发现
    pub finding: Option<Finding>,
    /// 是否通过
    pub passed: Option<bool>,
}

/// Coordinator 请求类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinatorRequestType {
    /// 初始化
    Initialize,
    /// 开始研究
    StartResearch,
    /// 添加发现
    AddFinding,
    /// 完成研究
    CompleteResearch,
    /// 开始实现
    StartImplementation,
    /// 分配任务
    AssignTask,
    /// 完成实现任务
    CompleteTask,
    /// 开始验证
    StartVerification,
    /// 完成验证
    CompleteVerification,
    /// 获取状态
    GetState,
    /// 重置
    Reset,
}

/// Coordinator 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorResponse {
    /// 是否成功
    pub success: bool,
    /// 阶段
    pub phase: Option<CoordinatorPhase>,
    /// 进度
    pub progress: Option<u32>,
    /// Worker IDs
    pub worker_ids: Option<Vec<String>>,
    /// 研究发现
    pub findings: Option<Vec<Finding>>,
    /// 实现规范
    pub specs: Option<Vec<ImplementationSpec>>,
    /// 错误
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_coordinator_initialization() {
        let mut coordinator = Coordinator::new();
        coordinator.initialize().await.unwrap();

        let phase = coordinator.get_phase().await;
        assert_eq!(phase, CoordinatorPhase::Research);
    }

    #[tokio::test]
    async fn test_research_phase() {
        let mut coordinator = Coordinator::new();
        coordinator.initialize().await.unwrap();

        let worker_ids = coordinator.start_research("Research this topic").await.unwrap();
        assert!(!worker_ids.is_empty());

        // 添加发现
        let finding = Finding {
            id: "finding_1".to_string(),
            finding_type: FindingType::Issue,
            description: "Found an issue".to_string(),
            evidence: vec!["evidence 1".to_string()],
            confidence: 0.9,
            source: "worker_1".to_string(),
        };
        coordinator.add_research_finding("research_worker_0", finding).await;

        let findings = coordinator.complete_research().await.unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[tokio::test]
    async fn test_synthesis_phase() {
        let mut coordinator = Coordinator::new();
        coordinator.initialize().await.unwrap();

        coordinator.start_research("Research").await.unwrap();

        let finding = Finding {
            id: "f1".to_string(),
            finding_type: FindingType::Suggestion,
            description: "Add this feature".to_string(),
            evidence: vec![],
            confidence: 0.8,
            source: "w1".to_string(),
        };
        coordinator.add_research_finding("research_worker_0", finding).await;
        coordinator.complete_research().await.unwrap();

        let specs = coordinator.synthesize().await.unwrap();
        assert!(!specs.is_empty());

        coordinator.complete_synthesis().await;
    }

    #[tokio::test]
    async fn test_implementation_phase() {
        let mut coordinator = Coordinator::new();
        coordinator.initialize().await.unwrap();

        coordinator.start_research("Research").await.unwrap();

        let finding = Finding {
            id: "f1".to_string(),
            finding_type: FindingType::Suggestion,
            description: "Implement this".to_string(),
            evidence: vec![],
            confidence: 0.8,
            source: "w1".to_string(),
        };
        coordinator.add_research_finding("research_worker_0", finding).await;
        coordinator.complete_research().await.unwrap();
        coordinator.complete_synthesis().await;

        let worker_ids = coordinator.start_implementation().await.unwrap();
        assert!(!worker_ids.is_empty());

        let state = coordinator.get_state().await;
        assert_eq!(state.phase, CoordinatorPhase::Implementation);
    }

    #[tokio::test]
    async fn test_full_flow() {
        let mut coordinator = Coordinator::new();
        coordinator.set_prompt("Build a web server".to_string());

        coordinator.initialize().await.unwrap();

        // Research
        coordinator.start_research("Research web servers").await.unwrap();
        let finding = Finding {
            id: "f1".to_string(),
            finding_type: FindingType::Architecture,
            description: "Use async".to_string(),
            evidence: vec!["Performance".to_string()],
            confidence: 0.95,
            source: "w1".to_string(),
        };
        coordinator.add_research_finding("research_worker_0", finding).await;
        coordinator.complete_research().await.unwrap();

        // Synthesis
        coordinator.synthesize().await.unwrap();
        coordinator.complete_synthesis().await;

        // Implementation
        coordinator.start_implementation().await.unwrap();
        coordinator.complete_implementation().await.unwrap();

        // Verification
        coordinator.start_verification().await.unwrap();
        coordinator.complete_verification(true).await.unwrap();

        assert!(coordinator.is_complete().await);

        let report = coordinator.generate_report().await;
        assert!(report.contains("Coordinator Report"));
    }
}
