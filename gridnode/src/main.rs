mod client;
mod config;
mod docker;

use crate::client::{ComputeHubClient, NodeRuntimeStatus, TaskConfig};
use crate::config::GridNodeConfig;
use crate::docker::DockerManager;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, sleep, Duration};
use tracing::{error, info, warn};

const CONFIG_PATH: &str = "/etc/idm-gridcore/gridnode.toml";
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
        let default_config = config::generate_default_config();
        std::fs::write(&config_path, default_config)?;
        info!("Created default config at {}", CONFIG_PATH);
        info!("Please edit the config file and restart");
        return Ok(());
    }

    // 加载配置
    let mut config = GridNodeConfig::from_file(&config_path)?;
    info!("Loaded config from {}", CONFIG_PATH);

    // 从配置文件读取 node_id（如果有的话）
    let existing_node_id = config.node_id.clone();

    // 根据架构确定 Docker platform
    let platform = match config.architecture.as_str() {
        "x86_64" => "linux/amd64",
        "aarch64" => "linux/arm64",
        "arm" => "linux/arm/v7",
        _ => "linux/amd64",
    };
    info!("Detected platform: {}", platform);

    // 创建客户端
    let client = ComputeHubClient::new(config.server_url.clone(), config.token.clone(), platform.to_string());

    // 注册节点（不传 node_id，让 ComputeHub 分配）
    let parallelism = config.get_parallelism();
    info!(
        "Registering node with {} CPUs (parallelism: {})",
        parallelism, parallelism
    );

    let register_resp = match client
        .register(
            existing_node_id.clone(),  // 首次为 None，后续为已有 ID
            config.hostname.clone(),
            config.architecture.clone(),
            parallelism,
        )
        .await
    {
        Ok(resp) => {
            info!("Registered successfully with node_id: {}", resp.node_id);
            
            // 如果是首次注册（配置文件没有 node_id），保存到配置文件
            if existing_node_id.is_none() {
                config.node_id = Some(resp.node_id.clone());
                config.save_to_file(&config_path)?;
                info!("Saved node_id to config file");
            }
            
            resp
        }
        Err(e) => {
            error!("Failed to register: {}", e);
            return Err(e);
        }
    };

    let node_id = register_resp.node_id;

    // 初始化 Docker 管理器
    let docker = match DockerManager::new() {
        Ok(d) => Arc::new(d),
        Err(e) => {
            error!("Failed to connect to Docker: {}", e);
            return Err(e);
        }
    };

    // 启动心跳任务
    let heartbeat_client = client.clone();
    let heartbeat_node_id = node_id.clone();
    let heartbeat_interval = config.heartbeat_interval;
    let active_containers = Arc::new(Mutex::new(0u32));
    let active_containers_for_heartbeat = active_containers.clone();

    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(heartbeat_interval));
        loop {
            interval.tick().await;
            let count = *active_containers_for_heartbeat.lock().await;
            let status = if count > 0 {
                NodeRuntimeStatus::Running
            } else {
                NodeRuntimeStatus::Idle
            };

            match heartbeat_client.heartbeat(&heartbeat_node_id, status, count).await {
                Ok(true) => {}
                Ok(false) => {
                    warn!("Heartbeat returned false, node may not be recognized");
                }
                Err(e) => {
                    warn!("Heartbeat failed: {}", e);
                }
            }
        }
    });

    // 主循环：管理容器
    let current_task: Arc<Mutex<Option<TaskConfig>>> = Arc::new(Mutex::new(register_resp.current_task));
    let current_task_for_watcher = current_task.clone();

    // 启动任务监控线程（轮询 ComputeHub 获取最新任务）
    let task_watcher_client = client.clone();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            match task_watcher_client.get_task().await {
                Ok(new_task) => {
                    let mut current = current_task_for_watcher.lock().await;
                    let changed = match (&*current, &new_task) {
                        (Some(old), Some(new)) => old.task_name != new.task_name,
                        (None, Some(_)) => true,
                        (Some(_), None) => true,
                        (None, None) => false,
                    };

                    if changed {
                        info!("Task changed: {:?} -> {:?}", current.as_ref().map(|t| &t.task_name), new_task.as_ref().map(|t| &t.task_name));
                        *current = new_task;
                    }
                }
                Err(e) => {
                    warn!("Failed to get task: {}", e);
                }
            }
        }
    });

    // 启动工作容器
    let mut container_handles = vec![];

    for instance_id in 0..parallelism {
        let docker = docker.clone();
        let current_task = current_task.clone();
        let active_containers = active_containers.clone();
        let node_id = node_id.clone();

        let handle = tokio::spawn(async move {
            let mut last_task_name: Option<String> = None;

            loop {
                // 获取当前任务
                let task_opt = current_task.lock().await.clone();

                match task_opt {
                    Some(task) => {
                        // 检查是否是新任务
                        let is_new_task = last_task_name.as_ref() != Some(&task.task_name);

                        if is_new_task {
                            info!("[Instance {}] Starting task '{}'", instance_id, task.task_name);
                            last_task_name = Some(task.task_name.clone());

                            // 拉取镜像
                            if let Err(e) = docker.pull_image(&task.image).await {
                                warn!("[Instance {}] Failed to pull image: {}", instance_id, e);
                            }

                            // 准备环境变量
                            let mut env = HashMap::new();
                            env.insert("TASK_NAME".to_string(), task.task_name.clone());
                            env.insert("NODE_ID".to_string(), node_id.clone());
                            env.insert("INSTANCE_ID".to_string(), instance_id.to_string());

                            if let Some(url) = task.input_redis {
                                env.insert("INPUT_REDIS_URL".to_string(), url);
                            }
                            if let Some(url) = task.output_redis {
                                env.insert("OUTPUT_REDIS_URL".to_string(), url);
                            }
                            if let Some(queue) = task.input_queue {
                                env.insert("INPUT_QUEUE".to_string(), queue);
                            }
                            if let Some(queue) = task.output_queue {
                                env.insert("OUTPUT_QUEUE".to_string(), queue);
                            }

                            // 启动容器
                            match docker
                                .start_container(
                                    &task.task_name,
                                    &task.image,
                                    &node_id,
                                    instance_id as usize,
                                    env,
                                )
                                .await
                            {
                                Ok(container_id) => {
                                    // 标记活跃
                                    *active_containers.lock().await += 1;

                                    // 等待容器完成
                                    let exit_code = docker.wait_container(&container_id).await.unwrap_or(-1);

                                    // 标记不活跃
                                    *active_containers.lock().await -= 1;

                                    if exit_code != 0 {
                                        warn!("[Instance {}] Container exited with code {}", instance_id, exit_code);
                                    }
                                }
                                Err(e) => {
                                    error!("[Instance {}] Failed to start container: {}", instance_id, e);
                                    sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                    }
                    None => {
                        // 没有任务，等待
                        if last_task_name.is_some() {
                            info!("[Instance {}] No task assigned, waiting...", instance_id);
                            last_task_name = None;
                        }
                        sleep(Duration::from_secs(5)).await;
                    }
                }

                // 短暂休息避免忙等
                sleep(Duration::from_millis(100)).await;
            }
        });

        container_handles.push(handle);
    }

    // 等待所有容器（实际上不会退出）
    for handle in container_handles {
        let _ = handle.await;
    }

    Ok(())
}
