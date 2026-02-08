# 交叉编译指南

本目录包含使用 `cross` 或 `cargo` 进行交叉编译的工具。

**适用场景**:
- **macOS** → 编译 Linux 二进制（必须用 cross）
- **Linux x86_64** → 编译 ARM64 二进制（用 cross）
- **Linux x86_64** → 编译 x86_64 二进制（直接用 cargo，脚本会自动优化）

## 前置要求

### macOS

1. **Rust** - 已安装 `cargo`
2. **Docker Desktop** - 必须运行（cross 使用 Docker 容器编译）
3. **cross** - 自动安装或手动安装：
   ```bash
   cargo install cross --git https://github.com/cross-rs/cross
   ```

### Linux

1. **Rust** - 已安装 `cargo`
2. **Docker** - 如果要交叉编译其他架构（如 ARM64）
   ```bash
   # Ubuntu/Debian
   sudo apt install docker.io
   sudo usermod -aG docker $USER
   ```

**注意**: 在 Linux x86_64 上编译本机目标时，脚本会自动使用 `cargo build` 而不是 `cross`，速度更快。

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

### macOS 主机

| 平台 | 目标 | 说明 |
|------|------|------|
| `linux-x64` | `x86_64-unknown-linux-gnu` | Linux 服务器 (Intel/AMD) |
| `linux-arm64` | `aarch64-unknown-linux-gnu` | 树莓派、ARM 云服务器 |
| `macos-arm64` | `aarch64-apple-darwin` | Apple Silicon Mac |
| `macos-x64` | `x86_64-apple-darwin` | Intel Mac |

### Linux 主机

| 平台 | 目标 | 说明 |
|------|------|------|
| `linux-x64` | `x86_64-unknown-linux-gnu` | Linux 服务器 (Intel/AMD) |
| `linux-arm64` | `aarch64-unknown-linux-gnu` | 树莓派、ARM 云服务器 |
| `macos-*` | - | ❌ **不支持**（macOS 闭源，无 Linux 工具链） |

**重要**: macOS 是闭源系统，只能在 macOS 上编译 macOS 目标。Linux 无法交叉编译 macOS 二进制。

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

**macOS**:
```
错误: Docker 未运行。cross 需要 Docker。
请先启动 Docker Desktop
```
**解决**: 启动 Docker Desktop

**Linux**:
```
错误: Docker 未运行。cross 需要 Docker。
请启动 docker 服务: sudo systemctl start docker
```
**解决**:
```bash
sudo systemctl start docker
# 或
sudo service docker start
```

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

## 平台特定说明

### macOS
必须使用 `cross` 编译 Linux 目标，因为 macOS 和 Linux 是不同的操作系统。

### Linux x86_64
- **编译本机目标** (`linux-x64`): 脚本自动使用 `cargo build`，速度最快
- **编译 ARM64 目标** (`linux-arm64`): 使用 `cross`，需要 Docker

示例：在 Linux x86_64 服务器上编译 ARM64 版本用于树莓派部署：
```bash
./scripts/build-cross.sh linux-arm64
scp dist/gridnode-linux-arm64 pi@raspberrypi:/usr/local/bin/
```

## 工作原理

### 使用 cross（跨 OS 或跨架构）
1. 运行 `cross build`
2. `cross` 启动对应平台的 Docker 容器
3. 容器内有完整的交叉编译工具链
4. 在容器内编译 Rust 项目
5. 输出二进制到 `target/<platform>/release/`

### 使用 cargo（Linux 本机）
1. 脚本检测到 Linux x86_64 编译本机目标
2. 直接使用 `cargo build --release`
3. 无需 Docker，编译速度更快

两种方式输出完全兼容的二进制文件。
