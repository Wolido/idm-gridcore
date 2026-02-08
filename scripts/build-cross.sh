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
    cat << EOF
IDM-GridCore 交叉编译工具

用法: $0 [选项] [平台...]

选项:
    -h, --help      显示帮助信息
    -l, --list      列出支持的平台
    -a, --all       编译所有平台（默认）
    -c, --clean     清理之前的构建

支持的平台:
    linux-x64       Linux x86_64 (Intel/AMD)
    linux-arm64     Linux ARM64 (树莓派、云服务器)
    macos-arm64     macOS ARM64 (Apple Silicon)
    macos-x64       macOS x86_64 (Intel Mac)

示例:
    $0                      # 编译所有平台
    $0 linux-x64            # 只编译 Linux x86_64
    $0 linux-x64 linux-arm64 # 编译多个平台
EOF
}

# 列出支持的平台
list_platforms() {
    echo "支持的平台:"
    echo "  linux-x64     Linux x86_64 (Intel/AMD 服务器)"
    echo "  linux-arm64   Linux ARM64 (树莓派、ARM 云服务器)"
    echo "  macos-arm64   macOS ARM64 (Apple Silicon M1/M2/M3)"
    echo "  macos-x64     macOS x86_64 (Intel Mac)"
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

echo -e "${GREEN}IDM-GridCore 交叉编译工具${NC}"
echo "========================================"

# 检查 cross 是否安装
if ! command -v cross &> /dev/null; then
    echo -e "${YELLOW}cross 未安装，正在安装...${NC}"
    cargo install cross --git https://github.com/cross-rs/cross
fi

# 检查 Docker 是否运行
if ! docker info &> /dev/null; then
    echo -e "${RED}错误: Docker 未运行。cross 需要 Docker。${NC}"
    echo "请先启动 Docker Desktop"
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

# 定义编译目标映射
# 格式: platform_name|target_triple
if [ ${#PLATFORMS[@]} -eq 0 ]; then
    # 默认编译所有平台
    TARGETS=(
        "linux-x64|x86_64-unknown-linux-gnu"
        "linux-arm64|aarch64-unknown-linux-gnu"
        "macos-arm64|aarch64-apple-darwin"
    )
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
                TARGETS+=("macos-arm64|aarch64-apple-darwin")
                ;;
            macos-x64)
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
        
        # 使用 cross 编译
        if cross build --release \
            --package "$CRATE" \
            --target "$TARGET" 2>&1 | tee /tmp/build-$CRATE-$PLATFORM.log; then
            
            # 复制并重命名二进制
            SRC="$PROJECT_ROOT/target/$TARGET/release/$BINARY"
            if [ -f "$SRC" ]; then
                DST="$OUTPUT_DIR/${BINARY}-${PLATFORM}"
                cp "$SRC" "$DST"
                chmod +x "$DST"
                
                # 显示文件大小
                SIZE=$(ls -lh "$DST" | awk '{print $5}')
                echo -e "    ${GREEN}✓${NC} ${BINARY}-${PLATFORM} (${SIZE})"
            else
                echo -e "    ${RED}✗ 编译成功但找不到二进制: $SRC${NC}"
            fi
        else
            echo -e "    ${RED}✗ 编译失败 (日志: /tmp/build-$CRATE-$PLATFORM.log)${NC}"
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
Build Tool: cross

Compiled Binaries:
EOF

ls -1 "$OUTPUT_DIR" | grep -v VERSION.txt >> "$OUTPUT_DIR/VERSION.txt"

echo ""
echo -e "${GREEN}编译完成！${NC}"
echo "输出目录: $OUTPUT_DIR"
echo ""
ls -lh "$OUTPUT_DIR"
