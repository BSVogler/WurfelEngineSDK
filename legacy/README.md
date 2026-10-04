# legacy

The first Rust port attempt (software-ish chunk renderer, native demo, wasm bindings). It is
superseded by `../wurfel-sim` + `../wurfel-web` and kept only for reference. It is its own cargo
workspace: `cd legacy && cargo check`. Known problems: the renderer duplicates itself across several
debug render paths, `Engine` wraps the renderer in `Arc<RefCell<..>>`, wgpu/winit are pinned to old
versions, `Coordinate::to_chunk` truncates toward zero for negative coordinates, and the README
claims more than the code does.
