# IDM-GridCore 技术实现文档

## 目录

1. [系统概述](#系统概述)
2. [架构设计](#架构设计)
3. [组件详解](#组件详解)
4. [数据流](#数据流)
5. [状态管理](#状态管理)
6. [容错机制](#容错机制)
7. [关键设计决策](#关键设计决策)

### 6. 为什么需要优雅停止？

**决策**: 任务切换和节点停止时，先发送 SIGTERM，超时后再 SIGKILL

**理由**:
- **数据完整性**: 给容器机会完成当前计算，避免任务数据丢失
- **可控性**: 超时时间可配置（默认 30 秒），平衡响应速度和数据安全
- **兼容性**: 支持需要完成当前循环才能退出的计算任务

### 7. 为什么支持远程和本地两种停止方式？

**决策**: 同时支持 API 远程停止和信号本地停止

**理由**:
- **灵活性**: 管理员可以从 ComputeHub 统一停止所有节点
- **便利性**: 用户也可以直接在计算节点上停止
- **兼容性**: 支持 systemd、Docker、K8s 等部署环境

---

## 系统概述

IDM-GridCore 是一个基于"众筹计算"理念的分布式并行计算框架。核心理念是：**计算节点匿名加入、批量并行执行、人工驱动切换**。

### 设计目标

| 目标 | 实现方式 |
|------|----------|
| 去中心化 | 计算节点只与 ComputeHub 通信，节点间无直接交互 |
| 异构支持 | 通过 Docker 抽象计算环境，支持任意 CPU 架构 |
| 边缘友好 | Rust 实现，内存占用低，树莓派可运行 |
| 动态扩缩 | 节点随时加入/退出，不影响任务执行 |
| 简化管理 | 人工确认任务完成，自动批量切换 |

### 6. 为什么需要优雅停止？

**决策**: 任务切换和节点停止时，先发送 SIGTERM，超时后再 SIGKILL

**理由**:
- **数据完整性**: 给容器机会完成当前计算，避免任务数据丢失
- **可控性**: 超时时间可配置（默认 30 秒），平衡响应速度和数据安全
- **兼容性**: 支持需要完成当前循环才能退出的计算任务

### 7. 为什么支持远程和本地两种停止方式？

**决策**: 同时支持 API 远程停止和信号本地停止

**理由**:
- **灵活性**: 管理员可以从 ComputeHub 统一停止所有节点
- **便利性**: 用户也可以直接在计算节点上停止
- **兼容性**: 支持 systemd、Docker、K8s 等部署环境

---

## 架构设计

### 整体架构

```
┌─────────────────────────────────────────────────────────────┐
│                         调度层                                │
│  ┌─────────────────┐      ┌─────────────────────────────┐   │
│  │  ComputeHub     │◄────►│  SQLite (任务队列、节点状态)  │   │
│  │  - HTTP API     │      └─────────────────────────────┘   │
│  │  - 状态管理      │                                        │
│  └────────┬────────┘                                        │
└───────────┼─────────────────────────────────────────────────┘
            │ HTTP
┌───────────┼─────────────────────────────────────────────────┐
│           ▼         计算层 (多节点)                            │
│  ┌─────────────────┐                                        │
│  │  GridNode       │  - 注册到 ComputeHub                   │
│  │  - 心跳维持      │  - 获取当前任务配置                     │
│  │  - 容器管理      │  - 启动 N 个 Docker 容器               │
│  └────────┬────────┘                                        │
│           │                                                  │
│  ┌────────┴────────┐                                        │
│  │  Docker 容器 x N │  - 每个容器单 CPU                       │
│  │  (N = CPU 核心数) │  - 从 Redis 取任务                     │
│  └────────┬────────┘  - 执行计算                             │
└───────────┼─────────────────────────────────────────────────┘
            │
┌───────────▼─────────────────────────────────────────────────┐
│                      数据层 (Redis)                          │
│  ┌─────────────────┐      ┌─────────────────────────────┐   │
│  │  Task1:Input    │      │  Task1:Output               │   │
│  │  Task2:Input    │      │  Task2:Output               │   │
│  └─────────────────┘      └─────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### 组件关系

- **ComputeHub**: 唯一的状态管理中心
- **GridNode**: 无状态，重启后重新注册
- **Redis**: 外部依赖，不存储在 ComputeHub
- **Docker**: 实际计算执行者

### 6. 为什么需要优雅停止？

**决策**: 任务切换和节点停止时，先发送 SIGTERM，超时后再 SIGKILL

**理由**:
- **数据完整性**: 给容器机会完成当前计算，避免任务数据丢失
- **可控性**: 超时时间可配置（默认 30 秒），平衡响应速度和数据安全
- **兼容性**: 支持需要完成当前循环才能退出的计算任务

### 7. 为什么支持远程和本地两种停止方式？

**决策**: 同时支持 API 远程停止和信号本地停止

**理由**:
- **灵活性**: 管理员可以从 ComputeHub 统一停止所有节点
- **便利性**: 用户也可以直接在计算节点上停止
- **兼容性**: 支持 systemd、Docker、K8s 等部署环境

---

## 组件详解

### 1. ComputeHub (服务端)

#### 职责

| 职责 | 说明 |
|------|------|
| 任务队列管理 | 维护任务列表，记录当前执行位置 |
| 节点注册管理 | 接受 GridNode 注册，分配 node_id |
| 心跳监控 | 维护节点在线状态，清理超时节点 |
| 任务配置分发 | 向 GridNode 返回当前任务配置 |
| 人工切换接口 | 接收切换指令，更新当前任务指针 |

#### 核心数据结构

```rust
// 应用状态（内存中，RwLock 保护）
AppStateInner {
    tasks: Vec<(Task, TaskStatus)>,      // 所有任务及其状态
    current_task_index: Option<usize>,   // 当前执行的任务索引
    nodes: HashMap<String, Node>,        // 在线节点
}

// 任务定义
Task {
    name: String,              // 任务标识
    image: String,             // Docker 镜像
    input_redis: Option<String>,   // 输入 Redis 地址（可选）
    output_redis: Option<String>,  // 输出 Redis 地址（可选）
    input_queue: Option<String>,   // 输入队列名（可选）
    output_queue: Option<String>,  // 输出队列名（可选）
}

// 节点信息
Node {
    id: String,                // 唯一标识
    hostname: String,          // 主机名
    architecture: String,      // CPU 架构
    cpu_count: u32,            // CPU 核心数
    last_seen: DateTime,       // 最后心跳时间
    status: NodeStatus,        // 在线/离线
    runtime_status: Option<NodeRuntimeStatus>,  // Running/Idle/Error
    active_containers: u32,    // 当前运行的容器数
    stop_requested: bool,      // 是否请求停止（优雅退出）
}
```

#### 状态转换

```
任务状态机:
Pending ──► Running ──► Completed
  ▲           │
  └───────────┘ (人工调用 next_task)
```

```
节点状态:
Online ◄──► Offline (心跳超时 60s)

运行时状态:
Idle ──► Running ──► Error (容器失败)
 ↑                      │
 └── 新任务/重置 ────────┘
```

#### 关键接口

**POST /api/tasks** - 注册任务
- 将任务添加到队列末尾
- 状态设为 Pending
- 如果这是第一个任务，不会自动开始（需调用 next）

**POST /api/tasks/next** - 切换到下一个任务
- 将当前任务标记为 Completed
- 找到下一个 Pending 任务，标记为 Running
- 更新 current_task_index
- 返回切换结果

**POST /gridnode/register** - 节点注册
- 如果请求中没有 node_id，ComputeHub 生成新的 UUID
- 如果请求中有 node_id，使用 GridNode 提供的 ID（用于重启恢复）
- 保存节点信息到 nodes
- 返回 node_id 和当前任务配置

**POST /gridnode/heartbeat** - 心跳
- 更新节点 last_seen
- 节点状态设为 Online
- 返回 `stop_requested` 标志，用于远程优雅退出

**GET /gridnode/task** - 获取任务配置
- 返回当前 Running 任务的配置
- GridNode 轮询此接口检测任务变化

**POST /api/nodes/{node_id}/stop** - 请求节点停止
- 管理员远程请求节点优雅退出
- 节点收到后在下次心跳时返回 stop_requested
- 节点停止所有容器后退出进程

#### 后台任务

**节点清理任务**（每 30 秒）
```rust
loop {
    sleep(30s);
    cleanup_nodes(timeout: 60s);
}
```

### 2. GridNode (计算节点)

#### 职责

| 职责 | 说明 |
|------|------|
| 服务发现 | 注册到 ComputeHub，获取 node_id |
| 心跳维持 | 定期发送心跳，保持在线状态 |
| 任务监控 | 轮询获取当前任务配置 |
| 容器管理 | 启动/停止 Docker 容器 |
| 资源控制 | 根据 CPU 数启动对应数量容器 |

#### 生命周期

```
启动
  │
  ▼
读取配置文件 (/etc/idm-gridcore/gridnode.toml)
  │
  ▼
注册到 ComputeHub
  - 发送 hostname, architecture, cpu_count
  - 接收 node_id（首次）和当前任务
  │
  ▼
启动三个并发任务：
  ├─ 心跳任务（定时发送）
  ├─ 任务监控任务（轮询任务变化）
  └─ 容器管理任务（每个 CPU 一个）
  │
  ▼
运行直到收到 SIGTERM/SIGINT 或远程停止命令
```

#### 容器管理流程

每个容器实例（共 N 个，N = CPU 核心数）：

```rust
loop {
    // 检查是否需要停止（本地信号或远程命令）
    if stop_requested {
        if has_running_container {
            docker.stop_container(timeout: 30s);  // 优雅停止
            docker.remove_container();
        }
        break;  // 退出工作线程
    }
    
    // 获取当前任务
    task = get_current_task();
    
    if task.is_none() {
        interruptible_sleep(5s);  // 可中断的等待
        continue;
    }
    
    if task.changed() {
        // 任务变化，拉取新镜像（带重试）
        docker.pull_image(task.image);
        
        // 准备环境变量
        env = {
            TASK_NAME: task.name,
            NODE_ID: node_id,
            INSTANCE_ID: instance_id,
            INPUT_REDIS_URL: task.input_redis,
            OUTPUT_REDIS_URL: task.output_redis,
            INPUT_QUEUE: task.input_queue,
            OUTPUT_QUEUE: task.output_queue,
        };
        
        // 启动容器
        container_id = docker.start_container(
            task.name,
            task.image,
            node_id,
            instance_id,
            env
        );
        
        // 等待容器完成或任务变化
        exit_code = wait_container_or_task_change(
            container_id, 
            timeout: 30s  // 优雅停止超时
        );
        
        if exit_code == -2 {
            // 任务变化导致的停止
            docker.stop_container(timeout: 30s);
            docker.remove_container();
        }
        
        // 容器退出后，循环继续
        // 如果任务没变，会重新启动容器（继续计算）
        // 如果任务变了，会拉取新镜像
    }
}
```

#### 任务变化检测与优雅停止

```rust
// 独立任务，每 10 秒轮询
loop {
    sleep(10s);
    new_task = computehub.get_task();
    
    if new_task.name != current_task.name {
        // 任务变化，通知所有容器管理协程
        // 使用 tokio::watch channel 实时广播
        task_tx.send(new_task);
    }
}
```

**优雅停止机制**:
- 任务切换或停止请求时，先发送 SIGTERM 给容器
- 容器有 30 秒（可配置）时间完成当前工作
- 超时后发送 SIGKILL 强制终止
- 停止后自动清理容器 (`docker rm`)

#### 配置管理

配置文件: `/etc/idm-gridcore/gridnode.toml`

```toml
server_url = "http://192.168.1.100:8080"
token = "your-secret-token"
# node_id = "..."  # 首次启动由 ComputeHub 分配，自动保存
hostname = "..." # 默认自动检测
architecture = "..." # 默认自动检测
parallelism = 4  # 可选，默认 CPU 核心数
heartbeat_interval = 30
stop_timeout = 30       # 停止容器的优雅超时（秒）
container_memory = 1024 # 每个容器的内存限制（MB）
```

**配置项说明**:
- `stop_timeout`: 任务切换或停止时，给容器多少秒时间优雅退出。如果容器需要完成当前循环，请设置足够长的时间。
- `container_memory`: 每个容器的内存限制（MB）。默认 1024MB (1GB)，可根据任务需求调整（512MB 轻量型，2048-4096MB 内存密集型）。

### 3. Docker 容器

#### 职责

- 执行实际计算任务
- 从 Redis 输入队列取任务
- 计算完成后写回 Redis 输出队列
- 内部自行实现阻塞等待逻辑

#### 环境变量

容器启动时注入的环境变量：

| 变量 | 说明 | 示例 |
|------|------|------|
| TASK_NAME | 任务名称 | hea-calc |
| NODE_ID | 节点 ID | uuid-string |
| INSTANCE_ID | 容器实例 ID | 0, 1, 2... |
| INPUT_REDIS_URL | 输入 Redis | redis://:pass@host:6379 |
| OUTPUT_REDIS_URL | 输出 Redis | redis://:pass@host:6379 |
| INPUT_QUEUE | 输入队列名 | task1:input |
| OUTPUT_QUEUE | 输出队列名 | task1:output |

#### 容器行为

容器内部逻辑（用户实现）：

```python
# 伪代码示例
import redis
import os

redis_url = os.getenv('INPUT_REDIS_URL')
input_queue = os.getenv('INPUT_QUEUE')
output_queue = os.getenv('OUTPUT_QUEUE')

r = redis.from_url(redis_url)

while True:
    # 阻塞等待任务
    task = r.blpop(input_queue, timeout=0)
    if not task:
        continue
    
    # 解析任务
    data = json.loads(task[1])
    
    # 执行计算
    result = compute(data)
    
    # 写回结果
    r.lpush(output_queue, json.dumps(result))
```

### 6. 为什么需要优雅停止？

**决策**: 任务切换和节点停止时，先发送 SIGTERM，超时后再 SIGKILL

**理由**:
- **数据完整性**: 给容器机会完成当前计算，避免任务数据丢失
- **可控性**: 超时时间可配置（默认 30 秒），平衡响应速度和数据安全
- **兼容性**: 支持需要完成当前循环才能退出的计算任务

### 7. 为什么支持远程和本地两种停止方式？

**决策**: 同时支持 API 远程停止和信号本地停止

**理由**:
- **灵活性**: 管理员可以从 ComputeHub 统一停止所有节点
- **便利性**: 用户也可以直接在计算节点上停止
- **兼容性**: 支持 systemd、Docker、K8s 等部署环境

---

## 数据流

### 完整数据流

```
1. 用户注册任务
   User ──► ComputeHub: POST /api/tasks
   ComputeHub: 保存任务到 tasks (Pending)

2. 用户启动任务
   User ──► ComputeHub: POST /api/tasks/next
   ComputeHub: 标记 Task1 为 Running

3. 用户推送任务数据
   User ──► Redis: LPUSH task1:input {task_data}
   (重复多次，推送所有任务)

4. 计算节点启动
   GridNode ──► ComputeHub: POST /gridnode/register
   ComputeHub: 返回 Task1 配置
   
5. GridNode 启动容器
   GridNode ──► Docker: start_container(Task1.image, env)
   Docker: 容器运行

6. 容器计算
   Container ──► Redis: BLPOP task1:input (阻塞等待)
   Redis: 返回任务数据
   Container: 计算
   Container ──► Redis: LPUSH task1:output result
   (循环，直到队列为空)

7. 人工确认完成
   User: 检查 Redis task1:input 为空
   User ──► ComputeHub: POST /api/tasks/next
   ComputeHub: Task1 Completed, Task2 Running

8. 任务切换
   GridNode (轮询): GET /gridnode/task
   ComputeHub: 返回 Task2 配置
   GridNode: 检测到任务变化
   GridNode: 停止 Task1 容器，启动 Task2 容器
   
9. 循环步骤 5-8
```

### 关键设计：为什么 Redis 队列独立？

- **解耦**: ComputeHub 不处理具体任务数据
- **灵活**: 用户可以用任何方式推送任务（脚本、CLI、其他服务）
- **可靠**: Redis 支持持久化，任务不会丢失
- **通用**: 不绑定特定数据格式

### 6. 为什么需要优雅停止？

**决策**: 任务切换和节点停止时，先发送 SIGTERM，超时后再 SIGKILL

**理由**:
- **数据完整性**: 给容器机会完成当前计算，避免任务数据丢失
- **可控性**: 超时时间可配置（默认 30 秒），平衡响应速度和数据安全
- **兼容性**: 支持需要完成当前循环才能退出的计算任务

### 7. 为什么支持远程和本地两种停止方式？

**决策**: 同时支持 API 远程停止和信号本地停止

**理由**:
- **灵活性**: 管理员可以从 ComputeHub 统一停止所有节点
- **便利性**: 用户也可以直接在计算节点上停止
- **兼容性**: 支持 systemd、Docker、K8s 等部署环境

---

## 状态管理

### 状态分布

| 状态 | 位置 | 说明 |
|------|------|------|
| 任务队列 | ComputeHub 内存 (RwLock) | 重启丢失，但任务配置可从 Redis 重建 |
| 节点列表 | ComputeHub 内存 | 临时状态，重启后节点重新注册 |
| 任务数据 | Redis | 持久化存储 |
| 节点配置 | GridNode 本地文件 | gridnode.toml，持久化 |
| 容器状态 | Docker Daemon | 由 Docker 管理 |

### 为什么 ComputeHub 不持久化到数据库？

设计选择：

1. **简化**: 内存状态足够，SQLite 只作为未来扩展点
2. **可恢复**: 任务队列可以从 Redis 重建（如果需要）
3. **轻量**: 单二进制部署，无外部依赖
4. **重启策略**: ComputeHub 重启后，GridNode 自动重新注册

### 状态恢复策略

**ComputeHub 重启**:
```
1. tasks 列表为空
2. nodes 列表为空
3. GridNode 心跳失败，重新注册
4. 用户重新调用 /api/tasks/next 开始任务
```

**GridNode 重启**:
```
1. 读取配置文件（保存了 node_id）
2. 重新注册到 ComputeHub
3. 获取当前任务，继续执行
```

### 6. 为什么需要优雅停止？

**决策**: 任务切换和节点停止时，先发送 SIGTERM，超时后再 SIGKILL

**理由**:
- **数据完整性**: 给容器机会完成当前计算，避免任务数据丢失
- **可控性**: 超时时间可配置（默认 30 秒），平衡响应速度和数据安全
- **兼容性**: 支持需要完成当前循环才能退出的计算任务

### 7. 为什么支持远程和本地两种停止方式？

**决策**: 同时支持 API 远程停止和信号本地停止

**理由**:
- **灵活性**: 管理员可以从 ComputeHub 统一停止所有节点
- **便利性**: 用户也可以直接在计算节点上停止
- **兼容性**: 支持 systemd、Docker、K8s 等部署环境

---

## 容错机制

### 1. 节点故障

**场景**: GridNode 崩溃或网络断开

**处理**:
- 心跳停止，ComputeHub 60s 后标记为 Offline
- 该节点正在运行的容器可能丢失任务（取决于 Redis 实现）
- 任务仍在 Redis 队列中，其他节点继续处理

**恢复**:
- 节点重启后重新注册，自动恢复计算

### 2. 容器故障

**场景**: 容器内计算失败退出

**处理**:
- GridNode 检测到容器退出
- 记录失败状态，通过心跳上报 Error 状态
- 如果任务没变，重新启动容器（继续计算）
- 如果任务已切换，启动新任务容器
- 连续失败会增加退避时间（可中断）

**注意**: 失败的任务需要用户自行处理重试（如重新推入队列）

### 3. ComputeHub 故障

**场景**: 服务端重启

**影响**:
- 任务队列丢失（需要重新注册任务）
- 节点需要重新注册

**恢复**:
- 服务重启后，节点自动重新连接
- 用户调用 /api/tasks/next 恢复任务

### 4. Redis 故障

**场景**: Redis 不可用

**影响**:
- 容器无法取任务/写结果
- 计算暂停

**恢复**:
- Redis 恢复后，计算自动继续

### 6. 为什么需要优雅停止？

**决策**: 任务切换和节点停止时，先发送 SIGTERM，超时后再 SIGKILL

**理由**:
- **数据完整性**: 给容器机会完成当前计算，避免任务数据丢失
- **可控性**: 超时时间可配置（默认 30 秒），平衡响应速度和数据安全
- **兼容性**: 支持需要完成当前循环才能退出的计算任务

### 7. 为什么支持远程和本地两种停止方式？

**决策**: 同时支持 API 远程停止和信号本地停止

**理由**:
- **灵活性**: 管理员可以从 ComputeHub 统一停止所有节点
- **便利性**: 用户也可以直接在计算节点上停止
- **兼容性**: 支持 systemd、Docker、K8s 等部署环境

---

## 关键设计决策

### 1. 为什么人工切换任务？

**决策**: 不由系统自动判断任务完成，而由人工调用 /api/tasks/next

**理由**:
- **简化**: 不需要复杂的完成检测逻辑
- **灵活**: 用户可以根据业务逻辑判断完成（如检查输出队列长度、结果质量等）
- **可控**: 避免误切换（如队列暂时为空但还有任务在传输中）

### 2. 为什么每个容器单 CPU？

**决策**: 每个 Docker 容器只绑定一个 CPU 核心

**理由**:
- **资源隔离**: 避免容器间争抢 CPU
- **简化设计**: 不需要在容器内管理多线程
- **可预测**: 性能稳定，便于估算总吞吐量

### 3. 为什么任务配置可选字段？

**决策**: Redis URL、队列名等都可以为空，由镜像内部决定

**理由**:
- **向后兼容**: 已有镜像不需要修改
- **灵活性**: 简单任务可以直接在镜像里写死配置
- **覆盖能力**: 需要多环境部署时可以覆盖

### 4. 为什么 Rust？

**决策**: 服务端和 GridNode 都用 Rust 实现

**理由**:
- **内存安全**: 避免内存泄漏，适合长期运行
- **性能**: 低内存占用，树莓派也能流畅运行
- **部署**: 单二进制，无运行时依赖

### 5. 为什么 HTTP 而不是 gRPC？

**决策**: 使用 RESTful HTTP API

**理由**:
- **简单**: 易于调试，curl 即可测试
- **防火墙友好**: 80/443 端口通常开放
- **生态**: Rust 的 axum/reqwest 成熟稳定

### 6. 为什么需要优雅停止？

**决策**: 任务切换和节点停止时，先发送 SIGTERM，超时后再 SIGKILL

**理由**:
- **数据完整性**: 给容器机会完成当前计算，避免任务数据丢失
- **可控性**: 超时时间可配置（默认 30 秒），平衡响应速度和数据安全
- **兼容性**: 支持需要完成当前循环才能退出的计算任务

### 7. 为什么支持远程和本地两种停止方式？

**决策**: 同时支持 API 远程停止和信号本地停止

**理由**:
- **灵活性**: 管理员可以从 ComputeHub 统一停止所有节点
- **便利性**: 用户也可以直接在计算节点上停止
- **兼容性**: 支持 systemd、Docker、K8s 等部署环境

---

## 扩展性设计

### 水平扩展

**计算节点扩展**:
- 启动新机器，安装 GridNode，自动加入
- 无上限（只受限于 ComputeHub 内存和网络）

**任务队列扩展**:
- 支持 Redis Cluster
- 可以分片存储任务数据

### 未来扩展点

| 功能 | 实现思路 |
|------|----------|
| GPU 支持 | 容器添加 --gpus 参数，配置中添加 gpu_count |
| 任务优先级 | Redis 使用不同优先级队列，或 Sorted Set |
| 自动完成检测 | 监控 Redis 队列长度，为空 N 分钟后自动切换 |
| 任务重试 | 失败任务写入 retry 队列，限制重试次数 |
| Web UI | 添加静态文件服务，展示节点状态、任务进度 |
| 认证授权 | 添加 JWT 中间件，区分用户权限 |

### 6. 为什么需要优雅停止？

**决策**: 任务切换和节点停止时，先发送 SIGTERM，超时后再 SIGKILL

**理由**:
- **数据完整性**: 给容器机会完成当前计算，避免任务数据丢失
- **可控性**: 超时时间可配置（默认 30 秒），平衡响应速度和数据安全
- **兼容性**: 支持需要完成当前循环才能退出的计算任务

### 7. 为什么支持远程和本地两种停止方式？

**决策**: 同时支持 API 远程停止和信号本地停止

**理由**:
- **灵活性**: 管理员可以从 ComputeHub 统一停止所有节点
- **便利性**: 用户也可以直接在计算节点上停止
- **兼容性**: 支持 systemd、Docker、K8s 等部署环境

---

## 性能考量

### 吞吐瓶颈

| 瓶颈点 | 优化策略 |
|--------|----------|
| ComputeHub | 无状态，可水平扩展（需共享状态） |
| Redis | 使用 Redis Cluster，或增加分片 |
| 网络带宽 | 任务数据本地化，减少传输 |
| 容器启动 | 预拉取镜像，使用镜像缓存 |

### 推荐配置

**小规模（< 100 节点）**:
- 1 台 ComputeHub（2核4G）
- 1 台 Redis（4核8G）
- 各计算节点运行 GridNode

**大规模（> 1000 节点）**:
- ComputeHub 多实例 + 负载均衡
- Redis Cluster
- 考虑添加任务分片

### 6. 为什么需要优雅停止？

**决策**: 任务切换和节点停止时，先发送 SIGTERM，超时后再 SIGKILL

**理由**:
- **数据完整性**: 给容器机会完成当前计算，避免任务数据丢失
- **可控性**: 超时时间可配置（默认 30 秒），平衡响应速度和数据安全
- **兼容性**: 支持需要完成当前循环才能退出的计算任务

### 7. 为什么支持远程和本地两种停止方式？

**决策**: 同时支持 API 远程停止和信号本地停止

**理由**:
- **灵活性**: 管理员可以从 ComputeHub 统一停止所有节点
- **便利性**: 用户也可以直接在计算节点上停止
- **兼容性**: 支持 systemd、Docker、K8s 等部署环境

---

## 总结

IDM-GridCore 通过以下设计实现简单高效的分布式计算：

1. **分层解耦**: 调度层、计算层、执行层分离
2. **无状态设计**: 节点故障可自愈
3. **人工驱动**: 简化自动化的复杂度
4. **容器化**: 屏蔽环境差异，支持异构部署
