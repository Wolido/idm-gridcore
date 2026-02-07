use serde::{Deserialize, Serialize};
use std::path::Path;

/// ComputeHub 配置文件
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    /// 监听地址
    #[serde(default = "default_bind")]
    pub bind: String,
    /// 节点认证 Token
    pub token: String,
}

fn default_bind() -> String {
    "0.0.0.0:8080".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            token: "change-me-in-production".to_string(),
        }
    }
}

impl ServerConfig {
    /// 从文件加载配置
    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: ServerConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// 保存配置到文件
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

/// 生成默认配置文件内容
pub fn generate_default_config() -> String {
    r#"# IDM-GridCore ComputeHub Configuration

# 监听地址
bind = "0.0.0.0:8080"

# 节点认证 Token（必须修改，用于验证 GridNode）
# GridNode 需要在配置中设置相同的 token 才能连接
token = "your-secret-token-change-this"
"#.to_string()
}
