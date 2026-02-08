FROM python:3.11-alpine

# 安装依赖
RUN pip install --no-cache-dir redis

# 复制消费者代码
WORKDIR /app
COPY consumer.py .

# 运行消费者
CMD ["python", "consumer.py"]
