mod audio;
mod bindings;
mod caveland_client;
mod interp;
mod lighting;
mod mesh;
mod minimap;
mod netstats;
mod pick;
mod particles;
mod prediction;
mod render_storage;

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("wurfel-web runs in the browser: use ./dev.sh (trunk serve), or run the tests with `cargo test -p wurfel-web`.");
}
