mod actors;
mod atlas;
mod audio;
mod bindings;
mod caveland_client;
mod editor;
mod interp;
mod lighting;
mod mesh;
mod minimap;
mod netstats;
mod pick;
mod sprites;
mod particles;
mod prediction;
mod render_storage;
mod texture;

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
