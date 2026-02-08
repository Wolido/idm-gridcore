#!/usr/bin/env python3
"""
IDM-GridCore 测试任务消费者
从 Redis 队列取数字，计算平方后放回结果队列
"""

import redis
import os
import time

# Redis 连接配置（GridNode 自动注入的环境变量）
INPUT_REDIS_URL = os.getenv("INPUT_REDIS_URL", "redis://localhost:6379")
OUTPUT_REDIS_URL = os.getenv("OUTPUT_REDIS_URL", INPUT_REDIS_URL)
INPUT_QUEUE = os.getenv("INPUT_QUEUE", "test:input")
OUTPUT_QUEUE = os.getenv("OUTPUT_QUEUE", "test:output")

# 实例标识（用于日志）
INSTANCE_ID = os.getenv("INSTANCE_ID", "0")
NODE_ID = os.getenv("NODE_ID", "unknown")[:8]  # 只取前8位便于显示


def main():
    print(f"[{NODE_ID}:{INSTANCE_ID}] Consumer starting...")
    print(f"  Input:  {INPUT_QUEUE}")
    print(f"  Output: {OUTPUT_QUEUE}")
    
    # 连接 Redis
    r_in = redis.from_url(INPUT_REDIS_URL)
    r_out = redis.from_url(OUTPUT_REDIS_URL)
    
    # 检查连接
    try:
        r_in.ping()
        print(f"[{NODE_ID}:{INSTANCE_ID}] ✓ Redis connected")
    except Exception as e:
        print(f"[{NODE_ID}:{INSTANCE_ID}] ✗ Redis connection failed: {e}")
        return
    
    processed = 0
    start_time = time.time()
    
    while True:
        try:
            # 阻塞等待任务（超时5秒，便于优雅退出）
            result = r_in.brpop(INPUT_QUEUE, timeout=5)
            
            if result is None:
                # 超时，检查队列是否为空
                if r_in.llen(INPUT_QUEUE) == 0:
                    elapsed = time.time() - start_time
                    print(f"[{NODE_ID}:{INSTANCE_ID}] Queue empty, exiting. Processed: {processed} in {elapsed:.1f}s")
                    break
                continue
            
            # 解析任务
            _, task_data = result
            n = int(task_data.decode() if isinstance(task_data, bytes) else task_data)
            
            # 计算: n -> n^2
            result = n * n
            
            # 写回结果（格式: "input:output"）
            output = f"{n}:{result}"
            r_out.lpush(OUTPUT_QUEUE, output)
            
            processed += 1
            
            # 每处理 1000 条打印一次进度
            if processed % 1000 == 0:
                elapsed = time.time() - start_time
                speed = processed / elapsed
                print(f"[{NODE_ID}:{INSTANCE_ID}] Progress: {processed:,} tasks @ {speed:.0f}/s")
                
        except Exception as e:
            print(f"[{NODE_ID}:{INSTANCE_ID}] Error: {e}")
            time.sleep(1)
    
    print(f"[{NODE_ID}:{INSTANCE_ID}] Done. Total processed: {processed}")


if __name__ == "__main__":
    main()
