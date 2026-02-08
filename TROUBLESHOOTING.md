# IDM-GridCore 故障排除指南

## 快速诊断

```bash
# 检查 ComputeHub 是否运行
curl http://localhost:8080/health

# 检查 GridNode 注册状态
curl -H "Authorization: Bearer your-token" http://localhost:8080/api/nodes

# 检查任务状态
curl -H "Authorization: Bearer your-token" http://localhost:8080/api/tasks

# 检查 Redis 队列长度
redis-cli -h redis-host -a password LLEN test:input
redis-cli -h redis-host -a password LLEN test:output
```

---

## Docker 权限问题

### 症状
```
❌ Docker 连接失败：权限不足！
```

### Linux 解决方案

**方案 1 - 将用户加入 docker 组（推荐，永久解决）：**
```bash
sudo usermod -aG docker $USER
newgrp docker  # 立即生效（或重新登录）
# 验证: docker ps  应该不需要 sudo
```

**方案 2 - 使用 sudo 运行 GridNode（临时）：**
```bash
sudo ./gridnode
```

**方案 3 - 检查 Docker 服务状态：**
```bash
sudo systemctl status docker
sudo systemctl start docker  # 如果未运行
```

### macOS 解决方案

**方案 1 - 使用 sudo 运行 GridNode（临时）：**
```bash
sudo ./gridnode
```

**方案 2 - 修复 Docker socket 权限：**
```bash
# Docker Desktop
sudo chown $USER ~/.docker/run/docker.sock

# OrbStack
sudo chown $USER ~/.orbstack/run/docker.sock
```

**方案 3 - 检查 Docker Desktop/OrbStack 是否运行：**
```bash
open -a Docker      # Docker Desktop
open -a OrbStack    # OrbStack
```

---

## GridNode 无法注册

### 症状
```
Failed to register: HTTP 401
```

### 原因与解决

1. **Token 不匹配**
   - 检查 `/etc/idm-gridcore/computehub.toml` 中的 `token`
   - 检查 `/etc/idm-gridcore/gridnode.toml` 中的 `token`
   - 两者必须完全一致

2. **无法连接到 ComputeHub**
   ```bash
   # 测试连通性
   curl http://<computehub-ip>:8080/health
   
   # 检查 GridNode 配置中的 server_url
   cat /etc/idm-gridcore/gridnode.toml
   ```

3. **防火墙阻止**
   - 确保计算节点能访问 ComputeHub 的端口（默认 8080）
   - 确保计算节点能访问 Redis 端口（默认 6379）

---

## 任务已注册但容器不启动

### 检查清单

1. **检查任务是否已启动**
   ```bash
   curl -H "Authorization: Bearer your-token" http://localhost:8080/api/tasks
   # 应该显示有 current 任务
   ```
   如果没有 current 任务，调用：
   ```bash
   curl -X POST -H "Authorization: Bearer your-token" http://localhost:8080/api/tasks/next
   ```

2. **检查镜像是否存在**
   ```bash
   docker images | grep your-image-name
   ```
   如果不存在，GridNode 会尝试自动拉取。

3. **检查 GridNode 日志**
   - 查看是否有 pull 镜像失败的错误
   - 查看是否有容器启动失败的错误

4. **手动测试容器**
   ```bash
   docker run --rm -e TASK_NAME=test your-image-name
   ```

---

## 任务执行但没有输出

### 检查清单

1. **检查 Redis 连接**
   ```bash
   # 在 GridNode 所在机器测试 Redis 连接
   redis-cli -h redis-host -a password PING
   ```

2. **检查输入队列是否有数据**
   ```bash
   redis-cli -h redis-host -a password LLEN your-input-queue
   ```

3. **检查容器日志**
   ```bash
   docker logs idm-taskname-nodeid-0
   ```

4. **检查环境变量**
   ```bash
   docker inspect idm-taskname-nodeid-0 | grep -A 20 Env
   ```
   确认以下变量正确设置：
   - INPUT_REDIS_URL
   - OUTPUT_REDIS_URL
   - INPUT_QUEUE
   - OUTPUT_QUEUE

---

## ComputeHub 重启后任务丢失

### 现象
ComputeHub 重启后，`/api/tasks` 返回空列表

### 原因
ComputeHub 状态保存在内存中，重启后丢失。

### 解决
重新注册任务：
```bash
curl -X POST http://localhost:8080/api/tasks \
  -H "Authorization: Bearer your-token" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "your-task",
    "image": "your-image",
    ...
  }'
```

然后切换到该任务：
```bash
curl -X POST -H "Authorization: Bearer your-token" http://localhost:8080/api/tasks/next
```

GridNode 会自动检测任务变化并继续执行。

---

## 跨平台编译问题

### Linux x86_64 编译失败

1. **安装依赖**
   ```bash
   # Debian/Ubuntu
   sudo apt-get install build-essential pkg-config libssl-dev
   ```

2. **使用 native-tls 而不是 rustls**
   GridNode 已配置使用 `native-tls-vendored`，避免 aws-lc-sys 编译问题。

### 交叉编译到 ARM64

使用提供的脚本：
```bash
./scripts/build-cross.sh linux-arm64
```

---

## 性能问题

### 吞吐量低于预期

1. **检查 CPU 利用率**
   ```bash
   top  # 或 htop
   ```
   每个容器应该占用约 100% CPU。

2. **检查 Redis 延迟**
   ```bash
   redis-cli --latency -h redis-host
   ```

3. **检查网络带宽**
   如果任务数据传输量大，确保网络不是瓶颈。

4. **调整并行度**
   在 `gridnode.toml` 中手动设置 `parallelism`：
   ```toml
   parallelism = 8  # 如果 CPU 有 8 核
   ```

---

## 日志查看

### ComputeHub
```bash
# 前台运行
cargo run --release 2>&1 | tee computehub.log

# 或使用 systemd
journalctl -u computehub -f
```

### GridNode
```bash
# 前台运行
./gridnode 2>&1 | tee gridnode.log

# 日志级别设置
RUST_LOG=debug ./gridnode  # 详细日志
RUST_LOG=info ./gridnode   # 默认级别
```

---

## 获取帮助

如果以上方法都无法解决问题：

1. 收集以下信息：
   - ComputeHub 版本 (`cargo pkgid`)
   - GridNode 运行日志
   - Docker 版本 (`docker version`)
   - 操作系统版本

2. 提交 Issue 时附上上述信息
