# IDM-GridCore

分布式并行计算调度系统 - 专为 IDM 团队设计的众筹式计算框架。

## 架构

```
┌─────────────────┐         ┌──────────────────┐
│  ComputeHub     │◄────────┤   Redis 队列      │
│  (调度服务端)    │   HTTP  │  (任务队列)       │
└────────┬────────┘         └──────────────────┘
         │
    ┌────┴────┬────────────┬────────────┐
    ▼         ▼            ▼            ▼
┌───────┐ ┌───────┐  ┌───────┐  ┌───────┐
│ Agent │ │ Agent │  │ Agent │  │ Agent │  ... 众筹节点
│Node-1 │ │Node-2 │  │Node-N │  │ ...   │
└───┬───┘ └───┬───┘  └───┬───┘  └───┬───┘
    │         │          │          │
┌───┴───┐ ┌───┴───┐  ┌───┴───┐  ┌───┴───┐
│Docker │ │Docker │  │Docker │  │Docker │
│ x N   │ │ x N   │  │ x N   │  │ x N   │
└───────┘ └───────┘  └───────┘ └───────┘
```

## 特点

- **去中心化**: 计算节点匿名加入，随时参与/退出
- **众筹计算**: 多节点齐头并进，完成一批再做下一批
- **异构支持**: 支持 x86_64, ARM64 等多种架构
- **轻量级**: 树莓派即可作为计算节点
- **无状态**: 节点故障不影响整体计算

## 快速开始

### 1. 启动 ComputeHub 服务端

```bash
cd server
cargo run --release
# 默认监听 0.0.0.0:8080
```

### 2. 在计算节点安装 Agent

```bash
cd agent
cargo build --release

# 首次运行生成配置文件
sudo mkdir -p /etc/idm-gridcore
sudo ./target/release/agent
# 编辑 /etc/idm-gridcore/agent.toml 配置服务端地址
```

### 3. 注册计算任务

```bash
# 使用 curl 注册任务
curl -X POST http://localhost:8080/api/tasks \
  -H "Content-Type: application/json" \
  -d '{
    "name": "hea-calc",
    "image": "your-registry/hea-calc:v1.0",
    "input_redis": "redis://:password@redis-host:6379",
    "output_redis": "redis://:password@redis-host:6379",
    "input_queue": "hea:input",
    "output_queue": "hea:output"
  }'
```

### 4. 启动计算节点

```bash
sudo ./target/release/agent
```

Agent 会自动：
- 向 ComputeHub 注册
- 根据 CPU 核心数启动 N 个容器
- 每个容器从 Redis 取任务计算
- 定期发送心跳

### 5. 人工切换任务

当第一个任务的 Redis 队列空了（人工确认）：

```bash
curl -X POST http://localhost:8080/api/tasks/next
```

所有计算节点会自动切换到下一个任务。

## API 文档

### 用户 API

| 接口 | 方法 | 说明 |
|------|------|------|
| `/api/tasks` | POST | 注册新任务 |
| `/api/tasks` | GET | 查看任务队列 |
| `/api/tasks/next` | POST | 切换到下一个任务 |
| `/api/nodes` | GET | 查看在线节点 |

### 计算节点 API

| 接口 | 方法 | 说明 |
|------|------|------|
| `/agent/register` | POST | 节点注册 |
| `/agent/heartbeat` | POST | 心跳上报 |
| `/agent/task` | GET | 获取当前任务配置 |

## 容器环境变量

计算容器启动时会注入以下环境变量：

| 变量 | 说明 |
|------|------|
| `TASK_NAME` | 任务名称 |
| `NODE_ID` | 节点 ID |
| `INSTANCE_ID` | 容器实例 ID |
| `INPUT_REDIS_URL` | 输入队列 Redis 地址 |
| `OUTPUT_REDIS_URL` | 输出队列 Redis 地址 |
| `INPUT_QUEUE` | 输入队列名 |
| `OUTPUT_QUEUE` | 输出队列名 |

容器内部使用这些变量连接 Redis 获取任务。

## 配置文件

### Agent 配置 (`/etc/idm-gridcore/agent.toml`)

```toml
# ComputeHub 服务端地址
server_url = "http://192.168.1.100:8080"

# 节点认证 Token
token = "your-secret-token"

# 主机名（默认自动检测）
hostname = "raspberry-pi-1"

# 并行容器数（默认使用 CPU 核心数）
parallelism = 4

# 心跳间隔（秒）
heartbeat_interval = 30
```

## 部署建议

### 服务端部署
- 建议部署在有公网 IP 的服务器上
- 使用 systemd 管理服务
- 配置 Nginx 反向代理（可选）

### 计算节点部署
- 树莓派、服务器、笔记本均可
- 需要 Docker 环境
- 需要能访问服务端和 Redis

### Redis 部署
- 建议部署在公网可访问的位置
- 使用密码认证
- 可考虑 Redis Cluster 高可用

## 开发

```bash
# 克隆仓库
git clone <repo>
cd idm-gridcore

# 编译
cargo build --release

# 测试
cargo test
```

## License

MIT
