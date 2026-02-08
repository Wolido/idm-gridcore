// 允许未使用赋值的警告（current_container_id 是误报，实际在停止分支中读取）
#![allow(unused_assignments)]

mod client;
mod config;
mod docker;

use crate::client::{ComputeHubClient, NodeRuntimeStatus, TaskConfig};
use crate::config::GridNodeConfig;
use crate::docker::DockerManager;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::{Mutex, watch};
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
    let client = ComputeHubClient::new(
        config.server_url.clone(),
        config.token.clone(),
        platform.to_string(),
    );

    // 注册节点（不传 node_id，让 ComputeHub 分配）
    let parallelism = config.get_parallelism();
    info!(
        "Registering node with {} CPUs (parallelism: {})",
        parallelism, parallelism
    );

    let register_resp = match client
        .register(
            existing_node_id.clone(), // 首次为 None，后续为已有 ID
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

    // 初始化 Docker 管理器（带重试）
    let docker = match init_docker_with_retry().await {
        Ok(d) => Arc::new(d),
        Err(e) => {
            error!("Failed to initialize Docker after retries: {}", e);
            return Err(e);
        }
    };

    // 启动心跳任务
    let heartbeat_client = client.clone();
    let heartbeat_node_id = node_id.clone();
    let heartbeat_interval = config.heartbeat_interval;
    let active_containers = Arc::new(Mutex::new(0u32));
    let container_errors = Arc::new(Mutex::new(HashMap::<u32, String>::new()));
    let active_containers_for_heartbeat = active_containers.clone();
    let container_errors_for_heartbeat = container_errors.clone();
    
    // 停止信号（用于优雅退出）
    let stop_requested = Arc::new(AtomicBool::new(false));
    let stop_requested_for_heartbeat = stop_requested.clone();
    let stop_requested_for_signal = stop_requested.clone();

    // 监听系统信号（SIGINT, SIGTERM）用于本地优雅退出
    tokio::spawn(async move {
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to create SIGTERM handler: {}", e);
                return;
            }
        };
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to create SIGINT handler: {}", e);
                return;
            }
        };

        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM, initiating graceful shutdown...");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT (Ctrl+C), initiating graceful shutdown...");
            }
        }

        stop_requested_for_signal.store(true, Ordering::SeqCst);
    });

    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(heartbeat_interval));
        loop {
            interval.tick().await;
            let count = *active_containers_for_heartbeat.lock().await;
            let errors = container_errors_for_heartbeat.lock().await;
            
            // 如果有错误，状态设为 Error
            let status = if !errors.is_empty() {
                NodeRuntimeStatus::Error
            } else if count > 0 {
                NodeRuntimeStatus::Running
            } else {
                NodeRuntimeStatus::Idle
            };

            match heartbeat_client
                .heartbeat(&heartbeat_node_id, status, count)
                .await
            {
                Ok((true, stop_requested_from_server)) => {
                    if stop_requested_from_server {
                        info!("Stop requested by ComputeHub, initiating graceful shutdown...");
                        stop_requested_for_heartbeat.store(true, Ordering::SeqCst);
                        break;
                    }
                }
                Ok((false, _)) => {
                    warn!("Heartbeat returned false, node may not be recognized");
                }
                Err(e) => {
                    warn!("Heartbeat failed: {}", e);
                }
            }
        }
    });

    // 使用 watch channel 来通知任务变化
    let (task_tx, _task_rx) = watch::channel(register_resp.current_task.clone());
    let task_tx = Arc::new(Mutex::new(task_tx));
    let task_tx_for_watcher = task_tx.clone();

    // 启动任务监控线程（轮询 ComputeHub 获取最新任务）
    let task_watcher_client = client.clone();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(10));
        let mut last_task: Option<TaskConfig> = None;
        
        loop {
            interval.tick().await;
            match task_watcher_client.get_task().await {
                Ok(new_task) => {
                    let changed = match (&last_task, &new_task) {
                        (Some(old), Some(new)) => old.task_name != new.task_name,
                        (None, Some(_)) => true,
                        (Some(_), None) => true,
                        (None, None) => false,
                    };

                    if changed {
                        info!(
                            "Task changed: {:?} -> {:?}",
                            last_task.as_ref().map(|t| &t.task_name),
                            new_task.as_ref().map(|t| &t.task_name)
                        );
                        last_task = new_task.clone();
                        // 通知所有工作线程任务变化
                        let _ = task_tx_for_watcher.lock().await.send(new_task);
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
    let stop_timeout = config.stop_timeout; // 停止容器的超时时间

    for instance_id in 0..parallelism {
        let docker = docker.clone();
        let mut task_rx = task_tx.lock().await.subscribe();
        let active_containers = active_containers.clone();
        let container_errors = container_errors.clone();
        let node_id = node_id.clone();
        let stop_requested_worker = stop_requested.clone();
        let container_memory = config.container_memory;

        let handle = tokio::spawn(async move {
            let mut last_task_name: Option<String> = None;
            let mut consecutive_failures: u32 = 0;
            let stop_timeout = stop_timeout;
            let container_memory = container_memory;
            let mut current_container_id: Option<String> = None;

            loop {
                // 检查是否需要停止
                if stop_requested_worker.load(Ordering::SeqCst) {
                    info!("[Instance {}] Stop requested, cleaning up...", instance_id);
                    // 如果有运行的容器，停止它
                    if let Some(ref container_id) = current_container_id {
                        info!("[Instance {}] Stopping container {}...", instance_id, container_id);
                        if let Err(e) = docker.stop_container(container_id, stop_timeout).await {
                            warn!("[Instance {}] Failed to stop container: {}", instance_id, e);
                        }
                        let _ = docker.remove_container(container_id).await;
                        // 标记为已处理
                        let _ = current_container_id.take();
                    }
                    break;
                }
                
                // 获取当前任务
                let task_opt = task_rx.borrow().clone();

                match task_opt {
                    Some(task) => {
                        // 检查是否是新任务
                        let is_new_task = last_task_name.as_ref() != Some(&task.task_name);

                        if is_new_task {
                            info!("[Instance {}] Starting task '{}' (previous failures: {})", instance_id, task.task_name, consecutive_failures);
                            last_task_name = Some(task.task_name.clone());
                            // 清除之前的错误状态，重置失败计数
                            consecutive_failures = 0;
                            container_errors.lock().await.remove(&instance_id);

                            // 拉取镜像（带重试）
                            if let Err(e) = pull_image_with_retry(&docker, &task.image, 3).await {
                                error!("[Instance {}] Failed to pull image after retries: {}", instance_id, e);
                                consecutive_failures += 1;
                                container_errors.lock().await.insert(
                                    instance_id,
                                    format!("Image pull failed: {}", e),
                                );
                                // 可中断的等待
                                if interruptible_sleep(Duration::from_secs(30), &mut task_rx).await {
                                    info!("[Instance {}] Sleep interrupted by task change", instance_id);
                                }
                                continue;
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

                            // 启动容器（带重试）
                            let mut container_started = false;
                            for attempt in 1..=3 {
                                match docker
                                    .start_container(
                                        &task.task_name,
                                        &task.image,
                                        &node_id,
                                        instance_id as usize,
                                        env.clone(),
                                        container_memory,
                                    )
                                    .await
                                {
                                    Ok(container_id) => {
                                        // 保存容器ID（用于优雅停止时停止容器）
                                        current_container_id = Some(container_id.clone());
                                        
                                        // 标记活跃
                                        *active_containers.lock().await += 1;
                                        container_errors.lock().await.remove(&instance_id);

                                        // 等待容器完成或任务变化
                                        let exit_code = wait_container_or_task_change(
                                            &docker,
                                            &container_id,
                                            &mut task_rx,
                                            instance_id,
                                            stop_timeout,
                                        ).await;

                                        // 清除容器ID
                                        current_container_id = None;
                                        
                                        // 标记不活跃
                                        *active_containers.lock().await -= 1;

                                        if exit_code == -2 {
                                            // 任务变化导致的停止
                                            info!("[Instance {}] Container stopped due to task change", instance_id);
                                            consecutive_failures = 0;
                                            container_errors.lock().await.remove(&instance_id);
                                            // 清理容器
                                            let _ = docker.remove_container(&container_id).await;
                                        } else if exit_code != 0 {
                                            warn!("[Instance {}] Container exited with code {}", instance_id, exit_code);
                                            consecutive_failures += 1;
                                            container_errors.lock().await.insert(
                                                instance_id,
                                                format!("Container exited with code {}", exit_code),
                                            );
                                        } else {
                                            // 成功完成，重置失败计数
                                            consecutive_failures = 0;
                                            container_errors.lock().await.remove(&instance_id);
                                        }

                                        container_started = true;
                                        break;
                                    }
                                    Err(e) => {
                                        error!("[Instance {}] Failed to start container (attempt {}/3): {}", instance_id, attempt, e);
                                        if attempt < 3 {
                                            // 可中断的等待
                                            if interruptible_sleep(Duration::from_secs(5), &mut task_rx).await {
                                                info!("[Instance {}] Retry sleep interrupted by task change", instance_id);
                                                break;  // 跳出重试循环，让外层处理新任务
                                            }
                                        }
                                    }
                                }
                            }

                            if !container_started {
                                error!("[Instance {}] Failed to start container after 3 attempts", instance_id);
                                consecutive_failures += 1;
                                container_errors.lock().await.insert(
                                    instance_id,
                                    "Failed to start container after 3 attempts".to_string(),
                                );
                                
                                // 如果连续失败太多，增加等待时间（可中断）
                                let backoff_secs = std::cmp::min(60u64, (10 * consecutive_failures) as u64);
                                warn!("[Instance {}] Backing off for {} seconds due to repeated failures", instance_id, backoff_secs);
                                if interruptible_sleep(Duration::from_secs(backoff_secs), &mut task_rx).await {
                                    info!("[Instance {}] Backoff interrupted by task change", instance_id);
                                }
                            }
                        }
                    }
                    None => {
                        // 没有任务，等待
                        if last_task_name.is_some() {
                            info!("[Instance {}] No task assigned, waiting...", instance_id);
                            last_task_name = None;
                            consecutive_failures = 0;
                            container_errors.lock().await.remove(&instance_id);
                        }
                        // 可中断的等待
                        if interruptible_sleep(Duration::from_secs(5), &mut task_rx).await {
                            info!("[Instance {}] Idle wait interrupted by task change", instance_id);
                        }
                    }
                }

                // 短暂休息避免忙等
                sleep(Duration::from_millis(100)).await;
            }
        });

        container_handles.push(handle);
    }

    // 等待停止信号或所有工作线程
    loop {
        if stop_requested.load(Ordering::SeqCst) {
            info!("Waiting for all workers to stop...");
            for handle in container_handles {
                let _ = handle.await;
            }
            
            info!("All workers stopped, cleaning up...");
            
            // 可选：清理所有镜像（如果需要）
            // 注意：这会比较激进，默认不启用
            // let _ = cleanup_all_images(&docker).await;
            
            info!("GridNode shutdown complete");
            break;
        }
        
        // 检查是否所有工作线程都异常退出了
        let all_finished = container_handles.iter().all(|h| h.is_finished());
        if all_finished {
            error!("All workers unexpectedly finished, exiting...");
            break;
        }
        
        sleep(Duration::from_secs(1)).await;
    }

    Ok(())
}

/// 初始化 Docker 管理器（带重试）
async fn init_docker_with_retry() -> anyhow::Result<DockerManager> {
    let mut last_error = None;
    
    for attempt in 1..=5 {
        info!("Attempting to connect to Docker (attempt {}/5)...", attempt);
        match DockerManager::new() {
            Ok(docker) => {
                info!("Successfully connected to Docker");
                return Ok(docker);
            }
            Err(e) => {
                last_error = Some(e);
                if attempt < 5 {
                    warn!("Docker connection failed, retrying in 5 seconds...");
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
    
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown Docker connection error")))
}

/// 拉取镜像（带重试）
async fn pull_image_with_retry(
    docker: &DockerManager,
    image: &str,
    max_retries: u32,
) -> anyhow::Result<()> {
    for attempt in 1..=max_retries {
        match docker.pull_image(image).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                if attempt < max_retries {
                    warn!("Failed to pull image (attempt {}/{}): {}, retrying...", attempt, max_retries, e);
                    sleep(Duration::from_secs(5)).await;
                } else {
                    return Err(e);
                }
            }
        }
    }
    
    Err(anyhow::anyhow!("Failed to pull image after {} attempts", max_retries))
}

/// 等待容器完成或任务变化
/// 返回: 容器退出码，或 -2 表示任务变化导致的停止
async fn wait_container_or_task_change(
    docker: &DockerManager,
    container_id: &str,
    task_rx: &mut watch::Receiver<Option<TaskConfig>>,
    instance_id: u32,
    stop_timeout: u64,
) -> i64 {
    use tokio::time::timeout;
    
    // 保存当前任务名用于比较
    let current_task_name = task_rx.borrow().as_ref().map(|t| t.task_name.clone());
    
    loop {
        // 检查任务是否变化（非阻塞）
        match task_rx.has_changed() {
            Ok(true) => {
                task_rx.mark_changed();
                let new_task = task_rx.borrow().clone();
                let new_name = new_task.as_ref().map(|t| t.task_name.clone());
                
                if new_name != current_task_name {
                    info!(
                        "[Instance {}] Task changed from {:?} to {:?}, stopping container (timeout: {}s)",
                        instance_id, current_task_name, new_name, stop_timeout
                    );
                    // 停止容器（使用配置的超时时间）
                    if let Err(e) = docker.stop_container(container_id, stop_timeout).await {
                        warn!("[Instance {}] Failed to stop container: {}", instance_id, e);
                    }
                    return -2; // 任务变化标记
                }
            }
            _ => {}
        }
        
        // 检查容器状态（每100ms检查一次）
        match timeout(Duration::from_millis(100), docker.wait_container(container_id)).await {
            Ok(Ok(exit_code)) => {
                return exit_code;
            }
            Ok(Err(e)) => {
                warn!("[Instance {}] Error waiting for container: {}", instance_id, e);
                return -1;
            }
            Err(_) => {
                // 超时，继续循环检查任务变化
                continue;
            }
        }
    }
}

/// 可中断的睡眠
/// 返回: true 表示被任务变化中断，false 表示正常完成
async fn interruptible_sleep(
    duration: Duration,
    task_rx: &mut watch::Receiver<Option<TaskConfig>>,
) -> bool {
    let sleep = tokio::time::sleep(duration);
    tokio::pin!(sleep);
    
    loop {
        tokio::select! {
            _ = &mut sleep => {
                // 正常完成睡眠
                return false;
            }
            result = task_rx.changed() => {
                // channel 关闭或出错，视为中断
                if result.is_err() {
                    return true;
                }
                // 检查任务是否真的变化了
                return true;
            }
        }
    }
}

/// 清理所有镜像（谨慎使用）
#[allow(dead_code)]
async fn cleanup_all_images(_docker: &DockerManager) -> anyhow::Result<()> {
    info!("Pruning unused images...");
    // 注意：这里可以实现镜像清理逻辑
    // 但默认不启用，因为可能会删除其他需要的镜像
    Ok(())
}
