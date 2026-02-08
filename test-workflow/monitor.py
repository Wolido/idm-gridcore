#!/usr/bin/env python3
"""
IDM-GridCore 测试监控工具
实时监控任务进度
"""

import redis
import os
import time
import sys

REDIS_URL = os.getenv("REDIS_URL", "redis://:changeme-strong-password@127.0.0.1:6379")
INPUT_QUEUE = "test:input"
OUTPUT_QUEUE = "test:output"


def main():
    print("IDM-GridCore Test Monitor")
    print("=" * 40)
    print()
    
    try:
        r = redis.from_url(REDIS_URL)
        r.ping()
    except Exception as e:
        print(f"Redis connection failed: {e}")
        sys.exit(1)
    
    # 获取初始状态
    initial_pending = r.llen(INPUT_QUEUE)
    initial_done = r.llen(OUTPUT_QUEUE)
    
    print(f"Initial state: {initial_pending:,} pending, {initial_done:,} done")
    print()
    print("Monitoring... (Ctrl+C to stop)")
    print()
    
    try:
        while True:
            pending = r.llen(INPUT_QUEUE)
            done = r.llen(OUTPUT_QUEUE)
            total = pending + done
            
            if total > 0:
                progress = done / total * 100
                print(f"\rPending: {pending:>10,}  |  Done: {done:>10,}  |  Progress: {progress:>5.1f}%", end="", flush=True)
            else:
                print(f"\rWaiting for tasks...", end="", flush=True)
            
            time.sleep(1)
            
    except KeyboardInterrupt:
        print("\n\nMonitoring stopped.")
        pending = r.llen(INPUT_QUEUE)
        done = r.llen(OUTPUT_QUEUE)
        print(f"Final: {pending:,} pending, {done:,} done")


if __name__ == "__main__":
    main()
