use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

/// ComputeHub 客户端（GridNode 使用）
#[derive(Clone)]
pub struct ComputeHubClient {
    client: Client,
    base_url: String,
    token: String,
    platform: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskConfig {
    pub task_name: String,
    pub image: String,
    pub redis_url: Option<String>,
    pub input_redis: Option<String>,
    pub output_redis: Option<String>,
    pub input_queue: Option<String>,
    pub output_queue: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterResponse {
    pub node_id: String,
    pub current_task: Option<TaskConfig>,
}

#[derive(Debug, Serialize)]
pub struct RegisterRequest {
    pub node_id: Option<String>,
    pub hostname: String,
    pub architecture: String,
    pub cpu_count: u32,
}

#[derive(Debug, Serialize)]
pub struct HeartbeatRequest {
    pub node_id: String,
    pub status: NodeRuntimeStatus,
    pub active_containers: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum NodeRuntimeStatus {
    Running,
    Idle,
    Error,
}

impl ComputeHubClient {
    pub fn new(base_url: String, token: String, platform: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
            token,
            platform,
        }
    }

    /// 注册节点
    pub async fn register(
        &self,
        node_id: Option<String>,
        hostname: String,
        architecture: String,
        cpu_count: u32,
    ) -> anyhow::Result<RegisterResponse> {
        let url = format!("{}/gridnode/register", self.base_url);
        let req = RegisterRequest {
            node_id,
            hostname,
            architecture,
            cpu_count,
        };

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&req)
            .send()
            .await?;

        if resp.status().is_success() {
            let data: RegisterResponse = resp.json().await?;
            Ok(data)
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            Err(anyhow::anyhow!(
                "Register failed: {} - {}",
                status,
                text
            ))
        }
    }

    /// 发送心跳
    pub async fn heartbeat(
        &self,
        node_id: &str,
        status: NodeRuntimeStatus,
        active_containers: u32,
    ) -> anyhow::Result<bool> {
        let url = format!("{}/gridnode/heartbeat", self.base_url);
        let req = HeartbeatRequest {
            node_id: node_id.to_string(),
            status,
            active_containers,
        };

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&req)
            .send()
            .await?;

        Ok(resp.status().is_success())
    }

    /// 获取当前任务
    pub async fn get_task(&self) -> anyhow::Result<Option<TaskConfig>> {
        let url = format!("{}/gridnode/task?platform={}", self.base_url, self.platform);

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await?;

        if resp.status().is_success() {
            let task: Option<TaskConfig> = resp.json().await?;
            Ok(task)
        } else {
            Err(anyhow::anyhow!(
                "Failed to get task: {}",
                resp.status()
            ))
        }
    }
}
