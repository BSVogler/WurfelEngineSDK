#!/bin/bash

# Build script for Wurfel Engine Rust port

set -e

echo "Building Wurfel Engine..."

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

print_step() {
    echo -e "${GREEN}[BUILD]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if Rust is installed
if ! command -v cargo &> /dev/null; then
    print_error "Rust/Cargo is not installed. Please install from https://rustup.rs/"
    exit 1
fi

# Check if wasm-pack is installed for WASM builds
if ! command -v wasm-pack &> /dev/null; then
    print_warning "wasm-pack not found. WASM builds will be skipped."
    print_warning "Install with: cargo install wasm-pack"
    SKIP_WASM=1
fi

# Build native version
print_step "Building native library..."
cargo build --workspace --exclude wurfel-wasm

print_step "Running tests..."
cargo test --workspace --exclude wurfel-wasm

# Build release version
print_step "Building release version..."
cargo build --release --workspace --exclude wurfel-wasm

# Build WASM version if wasm-pack is available
if [ -z "$SKIP_WASM" ]; then
    print_step "Building WASM version..."
    cd wurfel-wasm
    wasm-pack build --target web --out-dir pkg
    cd ..
    
    print_step "WASM build complete. Files generated in wurfel-wasm/pkg/"
    print_step "Serve wurfel-wasm/index.html with a local web server to test."
fi

print_step "Checking format..."
cargo fmt --all -- --check || {
    print_warning "Code formatting issues found. Run 'cargo fmt' to fix."
}

print_step "Running clippy..."
cargo clippy --all-targets --all-features -- -D warnings || {
    print_warning "Clippy warnings found. Please review and fix."
}

print_step "Build complete!"

echo ""
echo "Next steps:"
echo "  1. Run native demo: cd wurfel-demo && cargo run"
if [ -z "$SKIP_WASM" ]; then
    echo "  2. Test WASM build: serve wurfel-wasm/index.html locally"
    echo "     Example: python3 -m http.server 8000 (then visit http://localhost:8000/wurfel-wasm/)"
fi
echo "  3. Check documentation: cargo doc --open"