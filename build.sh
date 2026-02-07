#!/bin/bash
# Build script for IDM-GridCore
# Supports: Linux x86_64, Linux ARM64, macOS ARM64

set -e

echo "=== IDM-GridCore Multi-Platform Build ==="
echo ""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Build targets
build_linux_x64() {
    echo -e "${YELLOW}Building for Linux x86_64...${NC}"
    cargo build --release -p computehub -p gridnode
    mkdir -p dist/linux-x64
    cp target/release/computehub dist/linux-x64/
    cp target/release/gridnode dist/linux-x64/
    echo -e "${GREEN}✓ Linux x86_64 build complete${NC}"
    echo ""
}

build_linux_arm64() {
    echo -e "${YELLOW}Building for Linux ARM64...${NC}"
    
    # Check if cross-compiler is installed
    if ! command -v aarch64-linux-gnu-gcc &> /dev/null; then
        echo -e "${RED}✗ aarch64-linux-gnu-gcc not found${NC}"
        echo "Install with: sudo apt-get install gcc-aarch64-linux-gnu"
        return 1
    fi
    
    # Add target if not present
    rustup target add aarch64-unknown-linux-gnu 2>/dev/null || true
    
    cargo build --release --target aarch64-unknown-linux-gnu -p computehub -p gridnode
    mkdir -p dist/linux-arm64
    cp target/aarch64-unknown-linux-gnu/release/computehub dist/linux-arm64/
    cp target/aarch64-unknown-linux-gnu/release/gridnode dist/linux-arm64/
    echo -e "${GREEN}✓ Linux ARM64 build complete${NC}"
    echo ""
}

build_macos_arm64() {
    echo -e "${YELLOW}Building for macOS ARM64...${NC}"
    
    # Check if we're on macOS
    if [[ "$OSTYPE" != "darwin"* ]]; then
        echo -e "${YELLOW}⚠ Cross-compiling to macOS from Linux requires additional setup${NC}"
        echo "For now, please build directly on macOS with: cargo build --release"
        return 1
    fi
    
    # Add target if not present
    rustup target add aarch64-apple-darwin 2>/dev/null || true
    
    cargo build --release --target aarch64-apple-darwin -p computehub -p gridnode
    mkdir -p dist/macos-arm64
    cp target/aarch64-apple-darwin/release/computehub dist/macos-arm64/
    cp target/aarch64-apple-darwin/release/gridnode dist/macos-arm64/
    echo -e "${GREEN}✓ macOS ARM64 build complete${NC}"
    echo ""
}

# Main build logic
case "${1:-all}" in
    linux-x64)
        build_linux_x64
        ;;
    linux-arm64)
        build_linux_arm64
        ;;
    macos-arm64)
        build_macos_arm64
        ;;
    all)
        build_linux_x64
        build_linux_arm64 || echo -e "${YELLOW}⚠ Linux ARM64 build skipped${NC}"
        build_macos_arm64 || echo -e "${YELLOW}⚠ macOS ARM64 build skipped${NC}"
        ;;
    *)
        echo "Usage: $0 [linux-x64|linux-arm64|macos-arm64|all]"
        exit 1
        ;;
esac

echo "=== Build Summary ==="
if [ -d "dist" ]; then
    find dist -type f -exec ls -lh {} \;
else
    echo "No dist directory created"
fi
