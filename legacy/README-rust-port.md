# Wurfel Engine - Rust Port

A 2.5D isometric game engine ported from Java/libGDX to Rust with WebAssembly support. This engine focuses on chunk-based world rendering with an isometric projection, similar to the original Java version but with modern Rust performance and memory safety.

## Features

- **Chunk-based World System**: Efficient management of large 3D worlds
- **Isometric 2.5D Rendering**: Classic isometric projection with sprite-based rendering
- **Cross-platform**: Native desktop and web browser support via WebAssembly
- **Memory Safe**: Rust's ownership system eliminates common memory bugs
- **High Performance**: Zero-cost abstractions and efficient rendering pipeline
- **Block-based Voxel System**: Flexible block types with metadata support

## Architecture

### Core Components

- **Engine Core**: Main engine loop and configuration
- **Graphics System**: wgpu-based rendering with sprite batching
- **Map System**: Chunks, coordinates, blocks, and world generation
- **Math Utilities**: Isometric projection and coordinate conversion
- **Memory Management**: Object pooling for performance

### Project Structure

```
wurfel-engine/     # Core engine library
wurfel-wasm/       # WebAssembly bindings
wurfel-demo/       # Native demo application
```

## Getting Started

### Prerequisites

- Rust 1.70+ ([Install Rust](https://rustup.rs/))
- For WASM builds: `cargo install wasm-pack`

### Building

Run the build script to compile all components:

```bash
./build.sh
```

Or build manually:

```bash
# Build native library
cargo build --release

# Build WASM version
cd wurfel-wasm && wasm-pack build --target web
```

### Running the Demo

**Native Demo:**
```bash
cd wurfel-demo
cargo run
```

**Web Demo:**
1. Build the WASM version (see above)
2. Serve the HTML file locally:
   ```bash
   python3 -m http.server 8000
   # Visit http://localhost:8000/wurfel-wasm/
   ```

## Controls

- **WASD / Arrow Keys**: Move camera
- **Q/E**: Zoom in/out
- **Mouse Drag**: Pan camera (web version)
- **Mouse Wheel**: Zoom (web version)
- **ESC**: Exit (native version)

## Technical Details

### Coordinate System

The engine uses a 3D coordinate system with isometric projection:
- **X**: Left to right
- **Y**: Back to front  
- **Z**: Ground to sky

World coordinates are converted to screen coordinates using isometric projection formulas matching the original engine.

### Chunk System

Worlds are divided into chunks of 10×40×10 blocks. Chunks are loaded/unloaded dynamically and can be generated procedurally or loaded from storage.

### Block System

Blocks are represented as packed 16-bit values:
- Lower 8 bits: Block ID (type)
- Upper 8 bits: Block value (variant/metadata)

### Rendering Pipeline

1. **Chunk Traversal**: Iterate through visible chunks
2. **Sprite Generation**: Convert blocks to sprite instances  
3. **Depth Sorting**: Sort sprites by depth for proper rendering order
4. **Batch Rendering**: Render all sprites in efficient batches

### Memory Management

The engine uses object pooling for frequently allocated objects like RenderCells and chunks to minimize garbage collection pressure.

## Comparison with Original Java Version

| Feature | Java (libGDX) | Rust Port |
|---------|---------------|-----------|
| **Memory Safety** | GC + Manual | Ownership System |
| **Performance** | JIT Compiled | Native + Zero-cost |
| **Web Support** | Limited (GWT) | Full WASM Support |
| **Threading** | Manual Sync | Safe Concurrency |
| **Build System** | Maven | Cargo |
| **Package Size** | ~50MB+ | ~2MB WASM |

## MVP Status

This MVP demonstrates:
- [x] Basic chunk rendering
- [x] Isometric camera system
- [x] Block-based world structure
- [x] Cross-platform compilation (native + WASM)
- [x] Interactive camera controls
- [x] Procedural world generation
- [x] Memory-efficient chunk management

## Future Enhancements

Planned features for full engine parity:
- Entity system with components
- Lighting and shading system  
- Asset loading and management
- Audio system
- Network multiplayer support
- Advanced world generators
- Physics integration
- Scripting system (WASM modules)

## Development

### Code Organization

- `src/core.rs`: Engine initialization and main loop
- `src/graphics/`: Rendering system (camera, sprites, textures)
- `src/map/`: World system (chunks, blocks, coordinates)  
- `src/math.rs`: Mathematical utilities
- `src/utils.rs`: Memory management and utilities

### Building from Source

1. Clone the repository
2. Install Rust toolchain
3. Run `./build.sh` or use cargo directly
4. See build output for next steps

### Contributing

This is a port/demonstration project. For the original engine, see the Java codebase in this repository.

## License

This port maintains the same BSD 3-Clause license as the original Wurfel Engine.

## Acknowledgments

- Original Wurfel Engine by Benedikt Vogler
- Rust community for excellent tooling and libraries
- wgpu team for cross-platform graphics
- WebAssembly working group for enabling Rust in browsers