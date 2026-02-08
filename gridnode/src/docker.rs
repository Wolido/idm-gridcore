use bollard::container::{Config, CreateContainerOptions, StartContainerOptions, WaitContainerOptions};
use bollard::Docker;
use bollard::models::HostConfig;
use std::collections::HashMap;
use tracing::{info, error, warn};

/// Docker 管理器
pub struct DockerManager {
    docker: Docker,
    platform: String, // 如 "linux/arm64", "linux/amd64"
}

impl DockerManager {
    pub fn new() -> anyhow::Result<Self> {
        // 检测当前架构
        let arch = std::env::consts::ARCH;
        let platform = match arch {
            "x86_64" => "linux/amd64".to_string(),
            "aarch64" => "linux/arm64".to_string(),
            "arm" => "linux/arm/v7".to_string(),
            _ => {
                tracing::warn!("Unknown architecture: {}, defaulting to native", arch);
                format!("linux/{}", arch)
            }
        };
        tracing::info!("Detected platform: {} (architecture: {})", platform, arch);
        
        // 尝试连接 Docker
        let docker = Self::connect_docker()?;
        Ok(Self { docker, platform })
    }

    fn connect_docker() -> anyhow::Result<Docker> {
        // 首先尝试标准连接（Linux socket 或 Windows named pipe）
        match Docker::connect_with_local_defaults() {
            Ok(docker) => {
                info!("Connected to Docker");
                return Ok(docker);
            }
            Err(e) => {
                let err_msg = e.to_string();
                
                // 检查是否是权限问题
                if err_msg.contains("permission denied") {
                    return Err(Self::permission_error(&err_msg));
                }
                
                // Linux/macOS: 尝试显式连接 unix socket
                #[cfg(unix)]
                {
                    match Docker::connect_with_unix(
                        "/var/run/docker.sock",
                        120,
                        bollard::API_DEFAULT_VERSION,
                    ) {
                        Ok(docker) => {
                            info!("Connected to Docker via unix socket");
                            return Ok(docker);
                        }
                        Err(e2) => {
                            if e2.to_string().contains("permission denied") {
                                return Err(Self::permission_error(&e2.to_string()));
                            }
                        }
                    }
                }
                
                // macOS: 尝试 Docker Desktop / OrbStack socket 路径
                #[cfg(target_os = "macos")]
                {
                    let home = std::env::var("HOME").unwrap_or_default();
                    let macos_paths = [
                        format!("{}/.orbstack/run/docker.sock", home),  // OrbStack
                        format!("{}/.docker/run/docker.sock", home),    // Docker Desktop
                        "/var/run/docker.sock".to_string(),
                    ];
                    
                    for path in &macos_paths {
                        if std::path::Path::new(path).exists() {
                            match Docker::connect_with_unix(
                                path,
                                120,
                                bollard::API_DEFAULT_VERSION,
                            ) {
                                Ok(docker) => {
                                    info!("Connected to Docker via {}", path);
                                    return Ok(docker);
                                }
                                Err(_) => continue,
                            }
                        }
                    }
                }
                
                return Err(Self::connection_error(&e.to_string()));
            }
        }
    }

    fn permission_error(err_msg: &str) -> anyhow::Error {
        eprintln!("\n❌ Docker 连接失败：权限不足！");
        eprintln!("\n当前用户没有 Docker 访问权限。解决方案：");
        
        #[cfg(target_os = "linux")]
        {
            eprintln!("\n方案 1 - 将用户加入 docker 组（推荐）：");
            eprintln!("   sudo usermod -aG docker $USER");
            eprintln!("   newgrp docker  # 立即生效，或重新登录");
            eprintln!("\n方案 2 - 使用 sudo 运行（临时）：");
            eprintln!("   sudo ./gridnode");
        }
        
        #[cfg(target_os = "macos")]
        {
            eprintln!("\n方案 1 - 检查 Docker Desktop 是否运行：");
            eprintln!("   open -a Docker");
            eprintln!("\n方案 2 - 检查 socket 权限：");
            eprintln!("   ls -la ~/.docker/run/docker.sock");
        }
        
        eprintln!("\n方案 3 - 检查 Docker 服务是否运行：");
        eprintln!("   sudo systemctl status docker  # Linux");
        eprintln!("   # macOS: 检查 Docker Desktop 状态栏图标");
        
        anyhow::anyhow!("Docker permission denied: {}", err_msg)
    }

    fn connection_error(err_msg: &str) -> anyhow::Error {
        eprintln!("\n❌ Docker 连接失败！");
        eprintln!("\n请确保 Docker 已安装并正在运行：");
        
        #[cfg(target_os = "linux")]
        {
            eprintln!("\nLinux 安装指南：");
            eprintln!("  1. 安装 Docker: https://docs.docker.com/engine/install/");
            eprintln!("  2. 启动服务: sudo systemctl start docker");
            eprintln!("  3. 设置开机启动: sudo systemctl enable docker");
        }
        
        #[cfg(target_os = "macos")]
        {
            eprintln!("\nmacOS 安装指南：");
            eprintln!("  1. 安装 Docker Desktop: https://www.docker.com/products/docker-desktop/");
            eprintln!("  2. 启动 Docker Desktop 应用");
            eprintln!("  3. 等待状态栏图标显示 Docker 正在运行");
        }
        
        eprintln!("\n错误详情: {}", err_msg);
        anyhow::anyhow!("Docker not available")
    }

    /// 启动计算容器
    /// memory_mb: 内存限制（MB）
    pub async fn start_container(
        &self,
        task_name: &str,
        image: &str,
        node_id: &str,
        instance_id: usize,
        env_vars: HashMap<String, String>,
        memory_mb: u64,
    ) -> anyhow::Result<String> {
        let container_name = format!("idm-{}-{}-{}", task_name, node_id, instance_id);
        
        // 尝试创建容器
        match self.try_create_container(&container_name, image, &env_vars, memory_mb).await {
            Ok(container) => Ok(container),
            Err(e) => {
                // 如果容器已存在，删除后重试
                if e.to_string().contains("Conflict") {
                    warn!("Container {} already exists, removing and recreating", container_name);
                    let _ = self.docker.remove_container(&container_name, None).await;
                    self.try_create_container(&container_name, image, &env_vars, memory_mb).await
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn try_create_container(
        &self,
        container_name: &str,
        image: &str,
        env_vars: &HashMap<String, String>,
        memory_mb: u64,
    ) -> anyhow::Result<String> {
        // 准备环境变量
        let env: Vec<String> = env_vars
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        let config = Config {
            image: Some(image.to_string()),
            env: Some(env),
            host_config: Some(HostConfig {
                // 限制资源
                cpu_count: Some(1i64),
                memory: Some(memory_mb as i64 * 1024 * 1024), // MB 转 bytes
                ..Default::default()
            }),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: container_name,
            platform: Some(self.platform.as_str()),
        };

        // 创建容器
        let container = self.docker.create_container(Some(options), config).await?;
        let container_id = container.id;
        
        // 启动容器
        self.docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await?;

        info!(
            "Started container {} for task '{}' (instance {})",
            container_id, container_name, 0
        );

        Ok(container_id)
    }

    /// 等待容器完成
    pub async fn wait_container(&self, container_id: &str) -> anyhow::Result<i64> {
        let mut stream = self.docker.wait_container(container_id, None::<WaitContainerOptions<String>>);
        
        use futures::StreamExt;
        
        while let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    let exit_code = response.status_code;
                    if exit_code == 0 {
                        info!("Container {} exited successfully", container_id);
                    } else {
                        warn!("Container {} exited with code {}", container_id, exit_code);
                    }
                    return Ok(exit_code);
                }
                Err(e) => {
                    error!("Error waiting for container {}: {}", container_id, e);
                    return Err(e.into());
                }
            }
        }

        Err(anyhow::anyhow!("Wait stream ended unexpectedly"))
    }

    /// 拉取镜像（根据当前架构）
    /// 返回: Ok(()) 表示镜像已准备好（拉取成功或已存在）
    ///       Err 表示拉取失败（权限错误、镜像不存在等）
    pub async fn pull_image(&self, image: &str) -> anyhow::Result<()> {
        // 首先检查本地是否已存在该镜像
        if self.image_exists_locally(image).await {
            info!("Image {} already exists locally, skipping pull", image);
            return Ok(());
        }
        
        info!("Pulling image: {} for platform: {}", image, self.platform);
        
        let options = bollard::image::CreateImageOptions {
            from_image: image,
            platform: self.platform.as_str(),
            ..Default::default()
        };

        let mut stream = self.docker.create_image(Some(options), None, None);
        use futures::StreamExt;
        
        let mut has_error = false;
        let mut error_msg = String::new();

        while let Some(result) = stream.next().await {
            match result {
                Ok(_) => {}
                Err(e) => {
                    let msg = e.to_string();
                    // 镜像已存在不是错误
                    if msg.contains("not found") || msg.contains("pull access denied") {
                        has_error = true;
                        error_msg = msg;
                    } else if msg.contains("permission denied") {
                        has_error = true;
                        error_msg = msg;
                    } else {
                        // 其他错误可能是网络问题，记录下来但继续尝试
                        warn!("Non-fatal error pulling image: {}", msg);
                    }
                }
            }
        }

        // 如果有致命错误，返回错误
        if has_error {
            return Err(anyhow::anyhow!("Failed to pull image '{}': {}", image, error_msg));
        }

        info!("Image pull completed: {}", image);
        Ok(())
    }
    
    /// 检查镜像是否在本地存在
    async fn image_exists_locally(&self, image: &str) -> bool {
        match self.docker.list_images(None::<bollard::image::ListImagesOptions<String>>).await {
            Ok(images) => {
                for img in images {
                    let repo_tags: &Vec<String> = &img.repo_tags;
                    for tag in repo_tags {
                        if tag == image || tag.starts_with(&format!("{}:", image)) {
                            return true;
                        }
                    }
                }
                false
            }
            Err(_) => false,
        }
    }

    /// 停止容器
    /// timeout_secs: 优雅停止超时时间（秒），超过后强制 SIGKILL
    pub async fn stop_container(&self, container_id: &str, timeout_secs: u64) -> anyhow::Result<()> {
        info!("Stopping container {} (timeout: {}s)", container_id, timeout_secs);
        
        use bollard::container::StopContainerOptions;
        
        let options = StopContainerOptions {
            t: timeout_secs as i64,
        };
        
        match self.docker.stop_container(container_id, Some(options)).await {
            Ok(_) => {
                info!("Container {} stopped successfully", container_id);
                Ok(())
            }
            Err(e) => {
                // 容器可能已经不存在的错误可以忽略
                let msg = e.to_string();
                if msg.contains("No such container") || msg.contains("not found") {
                    info!("Container {} already stopped or removed", container_id);
                    Ok(())
                } else {
                    Err(e.into())
                }
            }
        }
    }

    /// 删除容器
    pub async fn remove_container(&self, container_id: &str) -> anyhow::Result<()> {
        use bollard::container::RemoveContainerOptions;
        
        let options = RemoveContainerOptions {
            force: true,
            ..Default::default()
        };
        
        match self.docker.remove_container(container_id, Some(options)).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("No such container") || msg.contains("not found") {
                    Ok(())
                } else {
                    Err(e.into())
                }
            }
        }
    }

    /// 清理已停止的容器（保留供未来使用）
    #[allow(dead_code)]
    pub async fn cleanup_stopped(&self) -> anyhow::Result<()> {
        let containers = self.docker.list_containers(None::<bollard::container::ListContainersOptions<String>>).await?;
        
        for container in containers {
            if let Some(names) = container.names {
                for name in names {
                    if name.starts_with("/idm-") {
                        if let Some(state) = &container.state {
                            if state == "exited" {
                                if let Some(id) = &container.id {
                                    let _ = self.docker.remove_container(id, None).await;
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
}
