#!/usr/bin/env python3
"""
IDM-GridCore 测试任务生成器
发送 100 万个数字到 Redis 输入队列
"""

import redis
import os
import sys
import time
from tqdm import tqdm

# Redis 连接配置（从环境变量读取，与 GridNode 注入的变量一致）
REDIS_URL = os.getenv("INPUT_REDIS_URL", "redis://:changeme-strong-password@127.0.0.1:6379")
INPUT_QUEUE = os.getenv("INPUT_QUEUE", "test:input")

# 任务数量
TOTAL_TASKS = 1_000_000
# 批量推送大小（提高性能）
BATCH_SIZE = 1000


def main():
    print(f"Connecting to Redis: {REDIS_URL.replace(os.getenv('INPUT_REDIS_URL', '').split('@')[0].split(':')[-1] if '@' in REDIS_URL else '', '***')}")
    print(f"Target queue: {INPUT_QUEUE}")
    print(f"Total tasks: {TOTAL_TASKS:,}")
    print()
    
    try:
        r = redis.from_url(REDIS_URL)
        r.ping()
        print("✓ Redis connected")
    except Exception as e:
        print(f"✗ Redis connection failed: {e}")
        sys.exit(1)
    
    # 清空旧队列（可选）
    r.delete(INPUT_QUEUE)
    print(f"✓ Queue cleared")
    print()
    
    # 批量推送
    start_time = time.time()
    batch = []
    
    for i in tqdm(range(TOTAL_TASKS), desc="Pushing tasks", unit="tasks"):
        # 数据格式: 简单的数字
        batch.append(str(i))
        
        if len(batch) >= BATCH_SIZE:
            r.lpush(INPUT_QUEUE, *batch)
            batch = []
    
    # 推送剩余数据
    if batch:
        r.lpush(INPUT_QUEUE, *batch)
    
    elapsed = time.time() - start_time
    speed = TOTAL_TASKS / elapsed
    
    print()
    print(f"✓ Done! {TOTAL_TASKS:,} tasks pushed in {elapsed:.2f}s ({speed:,.0f} tasks/s)")
    print(f"  Queue length: {r.llen(INPUT_QUEUE):,}")


if __name__ == "__main__":
    main()
