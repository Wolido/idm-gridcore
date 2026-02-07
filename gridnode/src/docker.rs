use anyhow::Context;
use bollard::container::{Config, CreateContainerOptions, StartContainerOptions, WaitContainerOptions};
use bollard::Docker;
use bollard::models::HostConfig;
use std::collections::HashMap;
use tracing::{info, error, warn};

/// Docker 管理器
pub struct DockerManager {
    docker: Docker,
}

impl DockerManager {
    pub fn new() -> anyhow::Result<Self> {
        // 尝试连接 Docker
        Self::connect_docker()
    }

    fn connect_docker() -> anyhow::Result<Self> {
        // 首先尝试标准连接（Linux socket 或 Windows named pipe）
        match Docker::connect_with_local_defaults() {
            Ok(docker) => {
                info!("Connected to Docker");
                return Ok(Self { docker });
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
                            return Ok(Self { docker });
                        }
                        Err(e2) => {
                            if e2.to_string().contains("permission denied") {
                                return Err(Self::permission_error(&e2.to_string()));
                            }
                        }
                    }
                }
                
                // macOS: 尝试 Docker Desktop socket 路径
                #[cfg(target_os = "macos")]
                {
                    let home = std::env::var("HOME").unwrap_or_default();
                    let macos_paths = [
                        format!("{}/.docker/run/docker.sock", home),
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
                                    return Ok(Self { docker });
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
    pub async fn start_container(
        &self,
        task_name: &str,
        image: &str,
        node_id: &str,
        instance_id: usize,
        env_vars: HashMap<String, String>,
    ) -> anyhow::Result<String> {
        let container_name = format!("idm-{}-{}-{}", task_name, node_id, instance_id);
        
        // 尝试创建容器
        match self.try_create_container(&container_name, image, &env_vars).await {
            Ok(container) => Ok(container),
            Err(e) => {
                // 如果容器已存在，删除后重试
                if e.to_string().contains("Conflict") {
                    warn!("Container {} already exists, removing and recreating", container_name);
                    let _ = self.docker.remove_container(&container_name, None).await;
                    self.try_create_container(&container_name, image, &env_vars).await
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
                memory: Some(512i64 * 1024 * 1024), // 512MB 默认限制
                ..Default::default()
            }),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: container_name,
            platform: None,
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

    /// 拉取镜像
    pub async fn pull_image(&self, image: &str) -> anyhow::Result<()> {
        info!("Pulling image: {}", image);
        
        let options = bollard::image::CreateImageOptions {
            from_image: image,
            ..Default::default()
        };

        let mut stream = self.docker.create_image(Some(options), None, None);
        use futures::StreamExt;

        while let Some(result) = stream.next().await {
            match result {
                Ok(_) => {}
                Err(e) => {
                    warn!("Error pulling image: {}", e);
                    // 不返回错误，因为镜像可能已存在
                }
            }
        }

        info!("Image pull completed: {}", image);
        Ok(())
    }

    /// 清理已停止的容器
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
