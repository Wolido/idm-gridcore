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
        match Docker::connect_with_local_defaults() {
            Ok(docker) => Ok(Self { docker }),
            Err(e) => {
                eprintln!("\n❌ Docker 连接失败！");
                eprintln!("\n请确保 Docker 已安装并正在运行：");
                eprintln!("  1. 安装 Docker: https://docs.docker.com/get-docker/");
                eprintln!("  2. 启动 Docker 服务:");
                eprintln!("     sudo systemctl start docker");
                eprintln!("  3. 将当前用户加入 docker 组（可选，避免 sudo）:");
                eprintln!("     sudo usermod -aG docker $USER");
                eprintln!("     # 然后重新登录或执行: newgrp docker");
                eprintln!("\n错误详情: {}\n", e);
                Err(anyhow::anyhow!("Docker not available"))
            }
        }
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
