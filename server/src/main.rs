mod models;

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use models::{
    AppState, AppStateInner, CreateTaskRequest, HeartbeatRequest, Node, NodeStatus,
    RegisterNodeRequest, RegisterNodeResponse, Task, TaskConfig, TaskListResponse,
    TaskWithStatus, TaskStatus,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{info, warn};
use chrono::Utc;

#[tokio::main]
async fn main() {
    // 初始化日志
    tracing_subscriber::fmt::init();

    // 初始化状态
    let state: AppState = Arc::new(RwLock::new(AppStateInner::new()));

    // 启动节点清理任务
    let cleanup_state = state.clone();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let mut state = cleanup_state.write().await;
            let before = state.nodes.len();
            state.cleanup_offline_nodes(60); // 60秒超时
            let after = state.nodes.len();
            if before != after {
                info!("Cleaned up {} offline nodes", before - after);
            }
        }
    });

    // 构建路由
    let app = Router::new()
        .route("/api/tasks", post(create_task).get(list_tasks))
        .route("/api/tasks/next", post(next_task))
        .route("/api/nodes", get(list_nodes))
        .route("/gridnode/register", post(register_node))
        .route("/gridnode/heartbeat", post(heartbeat))
        .route("/gridnode/task", get(get_current_task))
        .route("/health", get(health_check))
        .with_state(state);

    // 启动服务
    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    info!("IDM-GridCore Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ========== 用户 API ==========

/// 注册新任务
async fn create_task(
    State(state): State<AppState>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let name = req.name.clone();
    let task = Task {
        name: req.name,
        image: req.image,
        input_redis: req.input_redis,
        output_redis: req.output_redis,
        input_queue: req.input_queue,
        output_queue: req.output_queue,
    };

    let mut state = state.write().await;
    state.add_task(task);
    
    info!("Task '{}' registered", name);
    Ok(StatusCode::CREATED)
}

/// 列出所有任务
async fn list_tasks(State(state): State<AppState>) -> Json<TaskListResponse> {
    let state = state.read().await;
    
    let mut current = None;
    let mut pending = Vec::new();
    let mut completed = Vec::new();

    for (_idx, (task, status)) in state.tasks.iter().enumerate() {
        match status {
            TaskStatus::Running => current = Some(task.name.clone()),
            TaskStatus::Pending => pending.push(task.name.clone()),
            TaskStatus::Completed => completed.push(task.name.clone()),
        }
    }

    Json(TaskListResponse {
        current,
        pending,
        completed,
    })
}

/// 切换到下一个任务
async fn next_task(State(state): State<AppState>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut state = state.write().await;
    
    match state.next_task() {
        Some((prev, current)) => {
            info!("Switched from '{}' to '{}'", prev, current);
            Ok(Json(serde_json::json!({
                "previous": prev,
                "current": current,
            })))
        }
        None => Err((
            StatusCode::BAD_REQUEST,
            "No more tasks available".to_string(),
        )),
    }
}

/// 列出在线节点
async fn list_nodes(State(state): State<AppState>) -> Json<Vec<models::Node>> {
    let state = state.read().await;
    let nodes: Vec<_> = state.nodes.values().cloned().collect();
    Json(nodes)
}

// ========== 计算节点 API ==========

/// 节点注册
async fn register_node(
    State(state): State<AppState>,
    Json(req): Json<RegisterNodeRequest>,
) -> Json<RegisterNodeResponse> {
    let node_id = req.node_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    
    let node = Node {
        id: node_id.clone(),
        hostname: req.hostname,
        architecture: req.architecture,
        cpu_count: req.cpu_count,
        last_seen: Utc::now(),
        status: NodeStatus::Online,
    };

    let mut state = state.write().await;
    state.register_node(node);

    // 返回当前任务配置
    let current_task = state.get_current_task().map(|task| TaskConfig {
        task_name: task.name.clone(),
        image: task.image.clone(),
        redis_url: None, // 简化：让计算端从任务配置里组合
        input_redis: task.input_redis.clone(),
        output_redis: task.output_redis.clone(),
        input_queue: task.input_queue.clone(),
        output_queue: task.output_queue.clone(),
    });

    info!("Node '{}' registered with {} CPUs", node_id, req.cpu_count);

    Json(RegisterNodeResponse {
        node_id,
        current_task,
    })
}

/// 节点心跳
async fn heartbeat(
    State(state): State<AppState>,
    Json(req): Json<HeartbeatRequest>,
) -> StatusCode {
    let mut state = state.write().await;
    
    if state.update_heartbeat(&req.node_id) {
        StatusCode::OK
    } else {
        warn!("Heartbeat from unknown node: {}", req.node_id);
        StatusCode::NOT_FOUND
    }
}

/// 获取当前任务（非阻塞）
async fn get_current_task(
    State(state): State<AppState>,
) -> Json<Option<TaskConfig>> {
    let state = state.read().await;
    
    let config = state.get_current_task().map(|task| TaskConfig {
        task_name: task.name.clone(),
        image: task.image.clone(),
        redis_url: None,
        input_redis: task.input_redis.clone(),
        output_redis: task.output_redis.clone(),
        input_queue: task.input_queue.clone(),
        output_queue: task.output_queue.clone(),
    });

    Json(config)
}

/// 健康检查
async fn health_check() -> &'static str {
    "OK"
}
