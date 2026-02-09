#!/bin/bash
# IDM-GridCore 交叉编译脚本
# 在 macOS 上使用 cross 编译多平台二进制

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 帮助信息
show_help() {
    local CURRENT_OS=$(uname -s)
    # 根据当前系统显示支持的平台
    if [ "$CURRENT_OS" = "Darwin" ]; then
        SUPPORTED="    linux-x64       Linux x86_64 (Intel/AMD)
    linux-arm64     Linux ARM64 (树莓派、云服务器)
    macos-arm64     macOS ARM64 (Apple Silicon)
    macos-x64       macOS x86_64 (Intel Mac)"
    else
        SUPPORTED="    linux-x64       Linux x86_64 (Intel/AMD)
    linux-arm64     Linux ARM64 (树莓派、云服务器)
    (macOS 目标只能在 macOS 系统上编译)"
    fi
    
    cat << EOF
IDM-GridCore 交叉编译工具

用法: $0 [选项] [平台...]

选项:
    -h, --help      显示帮助信息
    -l, --list      列出支持的平台
    -a, --all       编译所有平台（默认）
    -c, --clean     清理之前的构建

支持的平台 (当前系统: $CURRENT_OS):
$SUPPORTED

示例:
    $0                      # 编译所有支持的平台
    $0 linux-x64            # 只编译 Linux x86_64
    $0 linux-x64 linux-arm64 # 编译多个平台

注意:
    macOS 目标是闭源系统，只能在 macOS 上编译
EOF
}

# 列出支持的平台
list_platforms() {
    local CURRENT_OS=$(uname -s)
    echo "支持的平台 (当前系统: $CURRENT_OS):"
    echo "  linux-x64     Linux x86_64 (Intel/AMD 服务器)"
    echo "  linux-arm64   Linux ARM64 (树莓派、ARM 云服务器)"
    if [ "$CURRENT_OS" = "Darwin" ]; then
        echo "  macos-arm64   macOS ARM64 (Apple Silicon M1/M2/M3)"
        echo "  macos-x64     macOS x86_64 (Intel Mac)"
    else
        echo ""
        echo "注意: macOS 目标只能在 macOS 系统上编译"
        echo "      （macOS 是闭源系统，无 Linux 工具链）"
    fi
}

# 解析参数
CLEAN=0
PLATFORMS=()

while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_help
            exit 0
            ;;
        -l|--list)
            list_platforms
            exit 0
            ;;
        -a|--all)
            PLATFORMS=()
            shift
            ;;
        -c|--clean)
            CLEAN=1
            shift
            ;;
        -*)
            echo "未知选项: $1"
            show_help
            exit 1
            ;;
        *)
            PLATFORMS+=("$1")
            shift
            ;;
    esac
done

# 检测操作系统
OS=$(uname -s)
ARCH=$(uname -m)

echo -e "${GREEN}IDM-GridCore 交叉编译工具${NC}"
echo "========================================"
echo "当前系统: $OS ($ARCH)"
echo ""

# 在 Linux x86_64 上，如果目标是本机，直接用 cargo build
USE_CROSS=1
if [ "$OS" = "Linux" ] && [ "$ARCH" = "x86_64" ]; then
    # 检查是否只需要编译本机平台
    if [ ${#PLATFORMS[@]} -eq 1 ] && [ "${PLATFORMS[0]}" = "linux-x64" ]; then
        echo -e "${YELLOW}检测到 Linux x86_64 编译本机目标，使用原生 cargo build（更快）${NC}"
        USE_CROSS=0
    fi
fi

# 检查 cross 是否安装（如果需要）
if [ $USE_CROSS -eq 1 ] && ! command -v cross &> /dev/null; then
    echo -e "${YELLOW}cross 未安装，正在安装...${NC}"
    cargo install cross --git https://github.com/cross-rs/cross
fi

# 检查 Docker（如果使用 cross）
if [ $USE_CROSS -eq 1 ] && ! docker info &> /dev/null; then
    echo -e "${RED}错误: Docker 未运行。cross 需要 Docker。${NC}"
    case $OS in
        Darwin)
            echo "请先启动 Docker Desktop"
            ;;
        Linux)
            echo "请启动 docker 服务: sudo systemctl start docker"
            ;;
    esac
    exit 1
fi

# 项目根目录
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_ROOT"

# 清理
if [ $CLEAN -eq 1 ]; then
    echo "清理之前的构建..."
    rm -rf "$PROJECT_ROOT/dist"
    cross clean
fi

# 创建输出目录
OUTPUT_DIR="$PROJECT_ROOT/dist"
mkdir -p "$OUTPUT_DIR"

# 获取版本信息
VERSION=$(grep '^version' "$PROJECT_ROOT/server/Cargo.toml" | head -1 | cut -d'"' -f2)
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
BUILD_TIME=$(date +%Y%m%d-%H%M%S)

echo "版本: $VERSION"
echo "Commit: $COMMIT"
echo "构建时间: $BUILD_TIME"
echo ""

# Linux 无法编译 macOS 目标（闭源系统，无公开工具链）
if [ "$OS" = "Linux" ]; then
    # 过滤掉 macOS 目标
    FILTERED_PLATFORMS=()
    for platform in "${PLATFORMS[@]}"; do
        if [[ "$platform" == macos-* ]]; then
            echo -e "${YELLOW}警告: Linux 无法编译 macOS 目标 '$platform'，已跳过${NC}"
            echo "macOS 只能在 macOS 系统上编译"
        else
            FILTERED_PLATFORMS+=("$platform")
        fi
    done
    PLATFORMS=("${FILTERED_PLATFORMS[@]}")
fi

# 定义编译目标映射
# 格式: platform_name|target_triple
if [ ${#PLATFORMS[@]} -eq 0 ]; then
    # 默认编译所有支持的平台
    if [ "$OS" = "Darwin" ]; then
        # macOS: 可以编译 Linux 和 macOS
        TARGETS=(
            "linux-x64|x86_64-unknown-linux-gnu"
            "linux-arm64|aarch64-unknown-linux-gnu"
            "macos-arm64|aarch64-apple-darwin"
        )
    else
        # Linux: 只能编译 Linux
        TARGETS=(
            "linux-x64|x86_64-unknown-linux-gnu"
            "linux-arm64|aarch64-unknown-linux-gnu"
        )
    fi
else
    # 用户指定平台
    TARGETS=()
    for platform in "${PLATFORMS[@]}"; do
        case $platform in
            linux-x64)
                TARGETS+=("linux-x64|x86_64-unknown-linux-gnu")
                ;;
            linux-arm64)
                TARGETS+=("linux-arm64|aarch64-unknown-linux-gnu")
                ;;
            macos-arm64)
                if [ "$OS" = "Linux" ]; then
                    echo -e "${RED}错误: Linux 无法编译 macOS 目标${NC}"
                    echo "请在 macOS 系统上编译 macOS 目标"
                    exit 1
                fi
                TARGETS+=("macos-arm64|aarch64-apple-darwin")
                ;;
            macos-x64)
                if [ "$OS" = "Linux" ]; then
                    echo -e "${RED}错误: Linux 无法编译 macOS 目标${NC}"
                    echo "请在 macOS 系统上编译 macOS 目标"
                    exit 1
                fi
                TARGETS+=("macos-x64|x86_64-apple-darwin")
                ;;
            *)
                echo -e "${RED}错误: 不支持的平台 '$platform'${NC}"
                list_platforms
                exit 1
                ;;
        esac
    done
fi

# 要编译的 crate
CRATES=("server" "gridnode")
CRATE_PATHS=("$PROJECT_ROOT/server/Cargo.toml" "$PROJECT_ROOT/gridnode/Cargo.toml")
BINARY_NAMES=("computehub" "gridnode")

echo "开始交叉编译..."
echo "目标平台: ${#TARGETS[@]} 个"
echo ""

for TARGET_SPEC in "${TARGETS[@]}"; do
    IFS='|' read -r PLATFORM TARGET <<< "$TARGET_SPEC"
    
    echo -e "${YELLOW}[$PLATFORM] 编译目标: $TARGET${NC}"
    
    for i in "${!CRATES[@]}"; do
        CRATE="${CRATES[$i]}"
        BINARY="${BINARY_NAMES[$i]}"
        
        echo "  → 编译 $CRATE..."
        
        # 选择编译工具
        if [ $USE_CROSS -eq 1 ]; then
            BUILD_CMD="cross"
        else
            BUILD_CMD="cargo"
        fi
        
        # 执行编译（使用 --manifest-path 替代 --package，cross 兼容性更好）
        $BUILD_CMD build --release \
            --manifest-path "${CRATE_PATHS[$i]}" \
            --target "$TARGET" 2>&1 | tee /tmp/build-$CRATE-$PLATFORM.log
        
        # 检查编译是否真正成功（检查日志中是否有错误）
        if grep -q "^error\|FAILED\|Compiling $CRATE.*error" /tmp/build-$CRATE-$PLATFORM.log 2>/dev/null; then
            echo -e "    ${RED}✗ 编译失败 (日志: /tmp/build-$CRATE-$PLATFORM.log)${NC}"
            continue
        fi
        
        # 复制并重命名二进制（包含版本号）
        SRC="$PROJECT_ROOT/target/$TARGET/release/$BINARY"
        if [ -f "$SRC" ]; then
            DST="$OUTPUT_DIR/${BINARY}-${VERSION}-${PLATFORM}"
            cp "$SRC" "$DST"
            chmod +x "$DST"
            
            # 显示文件大小
            SIZE=$(ls -lh "$DST" | awk '{print $5}')
            echo -e "    ${GREEN}✓${NC} ${BINARY}-${VERSION}-${PLATFORM} (${SIZE})"
        else
            echo -e "    ${RED}✗ 编译后找不到二进制: $SRC${NC}"
            # 调试：显示实际目录内容
            echo "    调试信息 - 检查目录结构:"
            ls -la "$PROJECT_ROOT/target/$TARGET/release/" 2>/dev/null | head -10 || echo "    目录不存在"
            
            # 尝试查找二进制文件（可能输出到其他位置）
            FOUND=$(find "$PROJECT_ROOT/target" -name "$BINARY" -type f -newer /tmp/build-$CRATE-$PLATFORM.log 2>/dev/null | head -1)
            if [ -n "$FOUND" ]; then
                echo "    发现二进制在其他位置: $FOUND"
                echo "    尝试复制..."
                DST="$OUTPUT_DIR/${BINARY}-${VERSION}-${PLATFORM}"
                cp "$FOUND" "$DST"
                chmod +x "$DST"
                SIZE=$(ls -lh "$DST" | awk '{print $5}')
                echo -e "    ${GREEN}✓${NC} ${BINARY}-${VERSION}-${PLATFORM} (${SIZE})"
            else
                echo -e "    ${YELLOW}! 编译可能成功，但未生成目标文件${NC}"
            fi
        fi
    done
    
    echo ""
done

# 生成版本信息
cat > "$OUTPUT_DIR/VERSION.txt" << EOF
IDM-GridCore Build Info
=======================
Version: $VERSION
Commit: $COMMIT
Build Time: $BUILD_TIME
Build Tool: $(if [ $USE_CROSS -eq 1 ]; then echo "cross"; else echo "cargo"; fi)

Compiled Binaries:
EOF

ls -1 "$OUTPUT_DIR" | grep -v VERSION.txt >> "$OUTPUT_DIR/VERSION.txt"

echo ""
echo "打包发布文件..."
echo ""

# 为每个平台打包
PACKAGED=()
for TARGET_SPEC in "${TARGETS[@]}"; do
    IFS='|' read -r PLATFORM TARGET <<< "$TARGET_SPEC"
    
    # 检查该平台的二进制是否存在
    HAS_FILES=0
    for BINARY in "${BINARY_NAMES[@]}"; do
        if [ -f "$OUTPUT_DIR/${BINARY}-${VERSION}-${PLATFORM}" ]; then
            HAS_FILES=1
            break
        fi
    done
    
    if [ $HAS_FILES -eq 1 ]; then
        PACKAGE_NAME="idm-gridcore-${VERSION}-${PLATFORM}"
        PACKAGE_DIR="$OUTPUT_DIR/$PACKAGE_NAME"
        mkdir -p "$PACKAGE_DIR"
        
        # 复制二进制到临时目录（去掉版本号，使用标准名称）
        for BINARY in "${BINARY_NAMES[@]}"; do
            SRC="$OUTPUT_DIR/${BINARY}-${VERSION}-${PLATFORM}"
            if [ -f "$SRC" ]; then
                cp "$SRC" "$PACKAGE_DIR/$BINARY"
                chmod +x "$PACKAGE_DIR/$BINARY"
            fi
        done
        
        # 创建 tar.gz
        tar -czf "$OUTPUT_DIR/${PACKAGE_NAME}.tar.gz" -C "$OUTPUT_DIR" "$PACKAGE_NAME"
        rm -rf "$PACKAGE_DIR"
        
        SIZE=$(ls -lh "$OUTPUT_DIR/${PACKAGE_NAME}.tar.gz" | awk '{print $5}')
        echo -e "${GREEN}✓${NC} ${PACKAGE_NAME}.tar.gz (${SIZE})"
        PACKAGED+=("$PACKAGE_NAME.tar.gz")
    fi
done

echo ""
echo -e "${GREEN}编译完成！${NC}"
echo "输出目录: $OUTPUT_DIR"
echo ""
echo "二进制文件:"
ls -lh "$OUTPUT_DIR" | grep -v "\.tar\.gz" | grep -v "VERSION.txt" | tail -n +2
echo ""
echo "发布包:"
ls -lh "$OUTPUT_DIR"/*.tar.gz 2>/dev/null || echo "  (无)"
