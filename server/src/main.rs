mod auth;
mod config;
mod models;

use axum::{
    extract::State,
    http::StatusCode,
    middleware,
    response::Json,
    routing::{get, post},
    Router,
};
use config::{generate_default_config, ServerConfig};
use models::{
    AppState, AppStateInner, CreateTaskRequest, HeartbeatRequest, Node, NodeStatus,
    RegisterNodeRequest, RegisterNodeResponse, Task, TaskConfig, TaskListResponse, TaskStatus,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

const CONFIG_PATH: &str = "/etc/idm-gridcore/computehub.toml";
const CONFIG_DIR: &str = "/etc/idm-gridcore";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    // 检查配置文件
    let config_path = PathBuf::from(CONFIG_PATH);
    if !config_path.exists() {
        // 创建默认配置文件
        std::fs::create_dir_all(CONFIG_DIR)?;
        let default_config = generate_default_config();
        std::fs::write(&config_path, default_config)?;
        info!("Created default config at {}", CONFIG_PATH);
        info!("Please edit the config file and set a secure token, then restart");
        return Ok(());
    }

    // 加载配置
    let server_config = ServerConfig::from_file(&config_path)?;
    info!("Loaded config from {}", CONFIG_PATH);
    info!("Bind address: {}", server_config.bind);

    // 检查默认 token
    if server_config.token == "change-me-in-production"
        || server_config.token == "your-secret-token-change-this"
    {
        warn!("WARNING: Using default token! Please change it in the config file for security.");
    }

    // 初始化状态
    let state: AppState = Arc::new(RwLock::new(AppStateInner::new(server_config.clone())));

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
    let protected_routes = Router::new()
        .route("/api/tasks", post(create_task).get(list_tasks))
        .route("/api/tasks/next", post(next_task))
        .route("/api/nodes", get(list_nodes))
        .route("/gridnode/register", post(register_node))
        .route("/gridnode/heartbeat", post(heartbeat))
        .route("/gridnode/task", get(get_current_task))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::auth_middleware));

    let app = Router::new()
        .route("/health", get(auth::health_check))
        .merge(protected_routes)
        .with_state(state);

    // 解析绑定地址
    let addr: SocketAddr = server_config.bind.parse()?;
    info!("IDM-GridCore ComputeHub listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
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
        images: req.images,
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
async fn next_task(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
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
    let node_id = req
        .node_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // 根据架构确定平台（在移动前使用）
    let platform = match req.architecture.as_str() {
        "x86_64" => "linux/amd64",
        "aarch64" => "linux/arm64",
        "arm" => "linux/arm/v7",
        _ => "linux/amd64", // 默认
    };

    let node = Node {
        id: node_id.clone(),
        hostname: req.hostname,
        architecture: req.architecture,
        cpu_count: req.cpu_count,
        last_seen: chrono::Utc::now(),
        status: NodeStatus::Online,
    };

    let mut state = state.write().await;
    state.register_node(node);

    // 返回当前任务配置（根据节点架构选择镜像）
    let current_task = state.get_current_task().and_then(|task| {
        let image = task.get_image_for_platform(platform)?;
        Some(TaskConfig {
            task_name: task.name.clone(),
            image,
            redis_url: None,
            input_redis: task.input_redis.clone(),
            output_redis: task.output_redis.clone(),
            input_queue: task.input_queue.clone(),
            output_queue: task.output_queue.clone(),
        })
    });

    info!(
        "Node '{}' registered with {} CPUs",
        node_id, req.cpu_count
    );

    Json(RegisterNodeResponse {
        node_id,
        current_task,
    })
}

/// 节点心跳
async fn heartbeat(State(state): State<AppState>, Json(req): Json<HeartbeatRequest>) -> StatusCode {
    let mut state = state.write().await;

    if state.update_heartbeat(&req.node_id) {
        StatusCode::OK
    } else {
        warn!("Heartbeat from unknown node: {}", req.node_id);
        StatusCode::NOT_FOUND
    }
}

/// 获取当前任务（非阻塞）
/// 查询参数 platform: 如 linux/amd64, linux/arm64
async fn get_current_task(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Json<Option<TaskConfig>> {
    let state = state.read().await;

    let platform: &str = params.get("platform").map(|s| s.as_str()).unwrap_or("linux/amd64");

    let config = state.get_current_task().and_then(|task| {
        let image = task.get_image_for_platform(platform)?;
        Some(TaskConfig {
            task_name: task.name.clone(),
            image,
            redis_url: None,
            input_redis: task.input_redis.clone(),
            output_redis: task.output_redis.clone(),
            input_queue: task.input_queue.clone(),
            output_queue: task.output_queue.clone(),
        })
    });

    Json(config)
}
