# IDM-GridCore 测试工作流

100 万个数字的平方计算测试，验证分布式计算框架的吞吐能力。

## 文件说明

| 文件 | 说明 |
|------|------|
| `producer.py` | 发送 100 万个数字到 Redis 输入队列 |
| `consumer.py` | 计算容器内运行的代码（计算 n²） |
| `Dockerfile` | 构建计算容器镜像 |
| `monitor.py` | 实时监控任务进度 |

## 快速开始

### 1. 启动 Redis

```bash
cd ../redis-setup
docker compose up -d
cd ../test-workflow
```

### 2. 构建计算镜像

```bash
docker build -t idm-test:square .
```

### 3. 注册测试任务到 ComputeHub

```bash
curl -X POST http://localhost:8080/api/tasks \
  -H "Authorization: Bearer your-token" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "square-test",
    "image": "idm-test:square",
    "input_redis": "redis://:your-password@redis-host:6379",
    "output_redis": "redis://:your-password@redis-host:6379",
    "input_queue": "test:input",
    "output_queue": "test:output"
  }'
```

### 4. 发送测试数据

```bash
# 安装依赖
pip install redis tqdm

# 发送 100 万个数字
python producer.py
```

### 5. 启动 GridNode

在计算节点上：

```bash
sudo gridnode
```

### 6. 监控进度

```bash
python monitor.py
```

## 数据格式

- **输入队列**: 纯数字字符串 (`"0"`, `"1"`, `"2"`...)
- **输出队列**: `"input:output"` 格式 (`"0:0"`, `"1:1"`, `"2:4"`...)

## 预期结果

- 100 万条数据全部处理完成
- 输出队列长度 = 100 万
- 可以通过多个 GridNode 并行加速
