# IDM-GridCore

[![Built with 小顺子](https://img.shields.io/badge/Built%20with-%E5%B0%8F%E9%A1%BA%E5%AD%90-3b82f6?style=flat-square&logo=robotframework&logoColor=white)](.)

**众筹式分布式并行计算框架** - 让成百上千的计算节点像众筹一样共同参与任务，轻松实现万亿级数据并行计算。

## 项目概述

IDM-GridCore 是一个专为大规模并行计算设计的轻量级调度框架。它解决了传统分布式计算中节点管理复杂、扩容困难的问题，采用"众筹计算"理念：任何设备（服务器、笔记本、树莓派）都可以匿名加入计算网络，按队列顺序批量执行任务。

### 核心能力

| 能力 | 说明 |
|------|------|
| **队列驱动** | 任务通过 Redis 队列分发，计算节点自主获取 |
| **批量执行** | 同一批任务全员并行，完成后再开始下一批 |
| **异构支持** | x86_64、ARM64 混合部署，统一调度 |
| **边缘友好** | 树莓派也能流畅运行，Rust 实现内存占用低 |
| **动态扩缩** | 节点随时加入/退出，不影响整体计算 |
| **无状态设计** | 节点故障不丢任务，自动重试 |

### 适用场景

- **科学计算** - 材料模拟、分子动力学、有限元分析
- **AI 推理** - 大规模样本批量预测
- **数据处理** - 日志分析、ETL 任务、格式转换
- **参数扫描** - 超参搜索、蒙特卡洛模拟

### 典型架构

**数据流**: 用户任务 → Redis 输入队列 → 计算节点池 → Redis 输出队列

**角色分工**:
- **ComputeHub**: 管理任务队列、协调计算节点、监控运行状态
- **GridNode**: 部署在各计算节点，拉取镜像、启动容器、上报心跳
- **Redis**: 任务队列存储，输入输出分离
- **Docker**: 实际执行计算任务，每个容器单 CPU 运行

## 架构

**三层架构**

顶层 - 调度层
    ComputeHub 服务端：管理任务队列，协调计算节点
    Redis 队列：存储输入任务和输出结果

中层 - 计算层
    GridNode 节点：部署在各计算设备上，与服务端通信
    节点类型：服务器、工作站、树莓派等异构设备

底层 - 执行层
    Docker 容器：每个容器绑定单个 CPU 核心
    容器数量：默认等于节点 CPU 核心数，可配置
    执行模式：从 Redis 取任务，计算，写回结果

## 快速开始

### 1. 启动 ComputeHub 服务端

```bash
cd server
cargo run --release
# 默认监听 0.0.0.0:8080
```

### 2. 在计算节点安装 GridNode

```bash
cd gridnode
cargo build --release

# 首次运行生成配置文件
sudo mkdir -p /etc/idm-gridcore
sudo ./target/release/gridnode
# 编辑 /etc/idm-gridcore/gridnode.toml 配置服务端地址
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
sudo ./target/release/gridnode
```

GridNode 会自动：
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
| `/gridnode/register` | POST | 节点注册 |
| `/gridnode/heartbeat` | POST | 心跳上报 |
| `/gridnode/task` | GET | 获取当前任务配置 |

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

### GridNode 配置 (`/etc/idm-gridcore/gridnode.toml`)

```toml
# ComputeHub 服务端地址
server_url = "http://192.168.1.100:8080"

# 节点认证 Token
token = "your-secret-token"

# 节点唯一 ID（首次启动由 ComputeHub 分配，自动保存）
# node_id = "xxx-xxx-xxx"

# 主机名（默认自动检测）
# hostname = "raspberry-pi-1"

# 并行容器数（默认使用 CPU 核心数）
# parallelism = 4

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

---

Built with ❤️ by [小顺子](https://github.com/Wolido)
