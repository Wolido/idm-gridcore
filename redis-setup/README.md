# IDM-GridCore Redis 快速部署

为 IDM-GridCore 分布式计算框架提供的 Redis 服务部署配置。

## 特点

- **Host 网络模式** - Linux 服务器最佳性能（无 NAT 开销）
- **数据持久化** - AOF 模式，每秒刷盘
- **无内存限制** - 适合大规模数据存储（确保服务器内存充足）
- **自动重启** - 除非手动停止，否则总是自动重启

## 快速开始

### 1. 配置密码

```bash
# 编辑 .env 文件，修改密码
vim .env
```

必须修改：`REDIS_PASSWORD`（不要使用默认值！）

### 2. 启动服务

```bash
docker compose up -d
```

### 3. 验证连接

```bash
# 本地测试
docker exec -it idm-redis redis-cli AUTH your-password PING

# 预期输出：PONG
```

## 常用命令

```bash
# 启动
docker compose up -d

# 查看日志
docker compose logs -f

# 停止
docker compose down

# 停止并删除数据（谨慎！）
docker compose down -v

# 重启
docker compose restart

# 进入 Redis CLI
docker exec -it idm-redis redis-cli -a your-password
```

## IDM-GridCore 连接配置

GridNode 配置文件 (`/etc/idm-gridcore/gridnode.toml`)：

```toml
# 使用本机 Redis（GridNode 和 Redis 在同一台机器）
# 任务注册时：
#   input_redis = "redis://:your-password@127.0.0.1:6379"
# 如果使用非默认端口，修改 URL 中的端口号
```

任务注册 API：

```bash
curl -X POST http://computehub:8080/api/tasks \
  -H "Authorization: Bearer your-token" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "my-task",
    "image": "my-compute-image:latest",
    "input_redis": "redis://:your-password@redis-host:6379",
    "output_redis": "redis://:your-password@redis-host:6379",
    # 注意：如果使用非默认端口，请修改 URL 中的端口号
    "input_queue": "task:input",
    "output_queue": "task:output"
  }'
```

## 网络模式说明

### Host 模式（当前配置，推荐 Linux 服务器使用）

**优点**：
- 性能最好（无端口映射开销）
- 配置简单，端口直接暴露
- 容器可以看到宿主机所有网卡

**缺点**：
- macOS/Windows Docker Desktop 不支持
- 端口可能与其他服务冲突

### 端口映射模式（兼容性更好）

如需在 macOS/Windows 使用，或需要运行多个 Redis 实例：

1. 注释掉 `network_mode: host`
2. 取消注释 `ports:` 部分
3. 重新启动

## 安全配置

### 1. 防火墙限制（重要！）

```bash
# 只允许特定 IP 访问（推荐）
ufw allow from 192.168.1.0/24 to any port 6379

# 或只允许特定 IP
ufw allow from 10.0.0.5 to any port 6379
```

### 2. 强密码

```bash
# 生成随机密码（20位）
openssl rand -base64 20
```

### 3. 不暴露公网

如果有公网访问需求，使用 SSH 隧道：

```bash
# 本地端口转发
ssh -L 6379:localhost:6379 user@redis-server
```

## 故障排除

### 端口被占用

```bash
# 检查 6379 端口占用
ss -tlnp | grep 6379

# 如需修改端口，编辑 .env 文件中的 REDIS_PORT
# 例如：REDIS_PORT=6380
```

### 内存不足

```bash
# 查看 Redis 内存使用
docker exec -it idm-redis redis-cli -a your-password INFO memory

# 当前配置无内存限制，如需限制可手动添加 --maxmemory 参数
# 到 docker-compose.yml 的 command 中
```

### 连接失败

```bash
# 检查容器状态
docker ps | grep idm-redis

# 查看日志
docker compose logs --tail 50

# 测试本地连接
docker exec -it idm-redis redis-cli -a your-password PING
```

## 数据备份

```bash
# 备份数据
docker exec -it idm-redis redis-cli -a your-password SAVE
docker cp idm-redis:/data/dump.rdb ./backup-$(date +%Y%m%d).rdb

# 恢复数据
docker cp ./backup-xxx.rdb idm-redis:/data/dump.rdb
docker compose restart
```

## 相关链接

- [IDM-GridCore 项目](../README.md)
- [Redis 官方文档](https://redis.io/documentation)
- [Docker Hub - Redis](https://hub.docker.com/_/redis)
