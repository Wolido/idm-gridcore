use serde::{Deserialize, Serialize};
use std::path::Path;

/// GridNode 配置文件
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GridNodeConfig {
    /// ComputeHub 服务端地址
    pub server_url: String,
    /// 节点认证 Token
    pub token: String,
    /// 节点唯一 ID（首次运行时生成，保存到文件）
    pub node_id: Option<String>,
    /// 本机主机名（默认自动检测）
    #[serde(default)]
    pub hostname: String,
    /// CPU 架构（默认自动检测）
    #[serde(default)]
    pub architecture: String,
    /// 并行容器数（默认使用 CPU 核心数）
    pub parallelism: Option<u32>,
    /// 心跳间隔（秒）
    pub heartbeat_interval: u64,
    /// 停止容器的优雅超时（秒）
    /// 任务切换时，给容器多少时间来完成当前工作
    #[serde(default = "default_stop_timeout")]
    pub stop_timeout: u64,
    /// 每个容器的内存限制（MB）
    #[serde(default = "default_container_memory")]
    pub container_memory: u64,
}

fn default_stop_timeout() -> u64 {
    30 // 默认30秒
}

fn default_container_memory() -> u64 {
    1024 // 默认1024MB (1GB)
}

impl Default for GridNodeConfig {
    fn default() -> Self {
        Self {
            server_url: "http://localhost:8080".to_string(),
            token: "default-token".to_string(),
            node_id: None,
            hostname: gethostname::gethostname().to_string_lossy().to_string(),
            architecture: std::env::consts::ARCH.to_string(),
            parallelism: None,
            heartbeat_interval: 30,
            stop_timeout: 30,      // 默认30秒
            container_memory: 1024, // 默认1024MB (1GB)
        }
    }
}

impl GridNodeConfig {
    /// 从文件加载配置
    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: GridNodeConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// 保存配置到文件
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// 获取并行度（默认 CPU 核心数）
    pub fn get_parallelism(&self) -> u32 {
        self.parallelism.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(1)
        })
    }
}

/// 生成默认配置文件内容
pub fn generate_default_config() -> String {
    r#"# IDM-GridCore GridNode Configuration

# ComputeHub 服务端地址
server_url = "http://localhost:8080"

# 节点认证 Token（与服务端配置匹配）
token = "your-secret-token"

# 节点唯一 ID（首次启动由 ComputeHub 分配，自动保存）
# node_id = ""

# 主机名（默认自动检测）
# hostname = "my-node"

# CPU 架构（默认自动检测）
# architecture = "x86_64"

# 并行容器数（默认使用 CPU 核心数）
# parallelism = 4

# 心跳间隔（秒）
heartbeat_interval = 30

# 停止容器的优雅超时（秒）
# 任务切换时，GridNode 会先发送 SIGTERM 给容器
# 如果容器在这个时间内没有退出，会发送 SIGKILL 强制终止
# 如果你的计算容器需要处理信号并完成当前工作，请设置足够长的时间
# stop_timeout = 30
"#.to_string()
}
