use crate::config::ServerConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};

/// 任务定义（支持多架构镜像）
/// 方式1: 单镜像（默认架构）
///   image: "myapp:latest"
/// 
/// 方式2: 多架构镜像映射
///   images: {
///     "linux/amd64": "myapp:latest-amd64",
///     "linux/arm64": "myapp:latest-arm64"
///   }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub name: String,
    /// 单镜像（向后兼容，所有架构使用同一镜像）
    pub image: Option<String>,
    /// 多架构镜像映射
    pub images: Option<HashMap<String, String>>,
    /// 可选：覆盖 Redis 输入连接
    pub input_redis: Option<String>,
    /// 可选：覆盖 Redis 输出连接（默认与输入相同）
    pub output_redis: Option<String>,
    /// 可选：覆盖输入队列名
    pub input_queue: Option<String>,
    /// 可选：覆盖输出队列名
    pub output_queue: Option<String>,
}

impl Task {
    /// 获取指定平台的镜像
    pub fn get_image_for_platform(&self, platform: &str) -> Option<String> {
        // 首先尝试从 images 映射中获取
        if let Some(ref images) = self.images {
            if let Some(image) = images.get(platform) {
                return Some(image.clone());
            }
        }
        // 回退到默认 image
        self.image.clone()
    }
}

/// 任务状态（内部使用）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
}

/// 带状态的任务（保留供未来 API 扩展使用）
#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct TaskWithStatus {
    #[serde(flatten)]
    pub task: Task,
    pub status: TaskStatus,
}

/// 计算节点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub hostname: String,
    pub architecture: String,
    pub cpu_count: u32,
    pub last_seen: DateTime<Utc>,
    pub status: NodeStatus,
    /// 运行时状态：Running/Idle/Error
    pub runtime_status: Option<NodeRuntimeStatus>,
    /// 活跃容器数量
    pub active_containers: u32,
    /// 是否请求停止（管理员优雅退出指令）
    #[serde(skip)]  // 不序列化到客户端
    pub stop_requested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    Online,
    Offline,
}

/// 注册节点请求
#[derive(Debug, Deserialize)]
pub struct RegisterNodeRequest {
    pub node_id: Option<String>,
    pub hostname: String,
    pub architecture: String,
    pub cpu_count: u32,
}

/// 节点注册响应
#[derive(Debug, Serialize)]
pub struct RegisterNodeResponse {
    pub node_id: String,
    pub current_task: Option<TaskConfig>,
}

/// 任务配置（返回给节点的）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskConfig {
    pub task_name: String,
    pub image: String,
    pub redis_url: Option<String>,
    pub input_redis: Option<String>,
    pub output_redis: Option<String>,
    pub input_queue: Option<String>,
    pub output_queue: Option<String>,
}

/// 心跳请求
#[derive(Debug, Deserialize)]
pub struct HeartbeatRequest {
    pub node_id: String,
    pub status: NodeRuntimeStatus,
    pub active_containers: u32,
}

/// 心跳响应
#[derive(Debug, Serialize)]
pub struct HeartbeatResponse {
    pub stop_requested: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum NodeRuntimeStatus {
    Running,
    Idle,
    Error,
}

/// 应用状态（共享）
pub type AppState = Arc<RwLock<AppStateInner>>;

#[derive(Debug)]
pub struct AppStateInner {
    /// 服务器配置
    pub config: ServerConfig,
    /// 所有注册的任务，按顺序
    pub tasks: Vec<(Task, TaskStatus)>,
    /// 当前运行任务的索引（None 表示未开始）
    pub current_task_index: Option<usize>,
    /// 在线节点
    pub nodes: HashMap<String, Node>,
}

impl AppStateInner {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config,
            tasks: Vec::new(),
            current_task_index: None,
            nodes: HashMap::new(),
        }
    }

    /// 获取当前任务
    pub fn get_current_task(&self) -> Option<&Task> {
        self.current_task_index.and_then(|idx| {
            self.tasks.get(idx).map(|(task, _)| task)
        })
    }

    /// 添加新任务到队列末尾
    pub fn add_task(&mut self, task: Task) {
        self.tasks.push((task, TaskStatus::Pending));
    }

    /// 切换到下一个任务（人工调用）
    /// 返回 (上一个任务名, 新任务名)
    pub fn next_task(&mut self) -> Option<(String, String)> {
        let prev_idx = self.current_task_index;
        
        // 先获取上一个任务的名字（在修改前）
        let prev_name = prev_idx
            .and_then(|i| self.tasks.get(i))
            .map(|(t, _)| t.name.clone())
            .unwrap_or_else(|| "none".to_string());
        
        // 标记上一个任务完成
        if let Some(idx) = prev_idx {
            if let Some((_, status)) = self.tasks.get_mut(idx) {
                *status = TaskStatus::Completed;
            }
        }

        // 找到下一个 pending 任务
        let next_idx = prev_idx.map(|i| i + 1).unwrap_or(0);
        
        if next_idx < self.tasks.len() {
            self.current_task_index = Some(next_idx);
            if let Some((task, status)) = self.tasks.get_mut(next_idx) {
                *status = TaskStatus::Running;
                return Some((prev_name, task.name.clone()));
            }
        }
        
        None
    }

    /// 注册或更新节点
    pub fn register_node(&mut self, node: Node) {
        self.nodes.insert(node.id.clone(), node);
    }

    /// 更新节点心跳（带运行时状态）
    pub fn update_heartbeat(
        &mut self,
        node_id: &str,
        runtime_status: NodeRuntimeStatus,
        active_containers: u32,
    ) -> bool {
        if let Some(node) = self.nodes.get_mut(node_id) {
            node.last_seen = Utc::now();
            node.status = NodeStatus::Online;
            node.runtime_status = Some(runtime_status);
            node.active_containers = active_containers;

            // 记录错误状态日志
            if matches!(runtime_status, NodeRuntimeStatus::Error) {
                tracing::warn!(
                    "Node {} reported error status (active containers: {})",
                    node_id,
                    active_containers
                );
            }

            true
        } else {
            false
        }
    }

    /// 清理超时节点
    pub fn cleanup_offline_nodes(&mut self, timeout_secs: i64) {
        let now = Utc::now();
        self.nodes.retain(|_, node| {
            let elapsed = now.signed_duration_since(node.last_seen).num_seconds();
            elapsed < timeout_secs
        });
    }
}

/// 注册任务请求
#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub name: String,
    pub image: Option<String>,
    pub images: Option<HashMap<String, String>>,
    pub input_redis: Option<String>,
    pub output_redis: Option<String>,
    pub input_queue: Option<String>,
    pub output_queue: Option<String>,
}

/// 任务列表响应
#[derive(Debug, Serialize)]
pub struct TaskListResponse {
    pub current: Option<String>,
    pub pending: Vec<String>,
    pub completed: Vec<String>,
}
