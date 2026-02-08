# 交叉编译指南

本目录包含在 macOS 上使用 `cross` 进行交叉编译的工具。

## 前置要求

1. **Rust** - 已安装 `cargo`
2. **Docker Desktop** - 必须运行（cross 使用 Docker 容器编译）
3. **cross** - 自动安装或手动安装：
   ```bash
   cargo install cross --git https://github.com/cross-rs/cross
   ```

## 快速开始

```bash
# 进入项目目录
cd idm-gridcore

# 编译所有平台
./scripts/build-cross.sh

# 输出在 dist/ 目录
dist/
├── computehub-linux-x64
├── computehub-linux-arm64
├── computehub-macos-arm64
├── gridnode-linux-x64
├── gridnode-linux-arm64
├── gridnode-macos-arm64
└── VERSION.txt
```

## 命令选项

```bash
# 显示帮助
./scripts/build-cross.sh --help

# 列出支持的平台
./scripts/build-cross.sh --list

# 只编译 Linux x86_64
./scripts/build-cross.sh linux-x64

# 编译多个平台
./scripts/build-cross.sh linux-x64 linux-arm64

# 清理并重新编译
./scripts/build-cross.sh --clean
```

## 支持的平台

| 平台 | 目标 | 说明 |
|------|------|------|
| `linux-x64` | `x86_64-unknown-linux-gnu` | Linux 服务器 (Intel/AMD) |
| `linux-arm64` | `aarch64-unknown-linux-gnu` | 树莓派、ARM 云服务器 |
| `macos-arm64` | `aarch64-apple-darwin` | Apple Silicon Mac |
| `macos-x64` | `x86_64-apple-darwin` | Intel Mac |

## 部署

### 部署到 Linux 服务器

```bash
# 从 MacBook 复制到服务器
scp dist/computehub-linux-x64 user@server:/usr/local/bin/computehub
scp dist/gridnode-linux-x64 user@server:/usr/local/bin/gridnode

# 在服务器上设置权限
ssh user@server 'chmod +x /usr/local/bin/{computehub,gridnode}'
```

### 部署到树莓派

```bash
scp dist/gridnode-linux-arm64 pi@raspberrypi:/usr/local/bin/gridnode
```

## 故障排除

### Docker 未运行
```
错误: Docker 未运行。cross 需要 Docker。
```
**解决**: 启动 Docker Desktop

### 交叉编译失败
```bash
# 检查 cross 版本
cross --version

# 更新 cross
cargo install cross --git https://github.com/cross-rs/cross --force

# 检查 Docker 镜像
docker pull ghcr.io/cross-rs/x86_64-unknown-linux-gnu:latest
```

### 权限问题
```bash
chmod +x scripts/build-cross.sh
```

## 工作原理

`cross` 工具使用 Docker 容器提供完整的交叉编译环境：

1. 你的 MacBook 上运行 `cross build`
2. `cross` 启动对应平台的 Docker 容器
3. 容器内有完整的工具链（gcc, libc, 等）
4. 在容器内编译 Rust 项目
5. 输出二进制到 `target/<platform>/release/`

这种方式比本地安装交叉编译工具链更简单可靠。
