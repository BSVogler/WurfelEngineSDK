// The game runs in the browser (wasm). Natively this binary only exists to run the tests, so most of the
// code looks unused there.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

mod actors;
mod animation;
mod atlas;
mod atmosphere;
mod audio;
mod bindings;
mod caveland_client;
mod clouds;
mod cursor;
mod damage;
mod detail;
mod editor;
mod grass;
#[cfg(target_arch = "wasm32")]
mod gputime;
mod interp;
mod lightdebug;
mod lighting;
mod locator;
mod mesh;
mod minimap;
mod mode;
mod model;
mod netstats;
mod pick;
mod sprites;
mod spriteshadow;
mod particles;
mod peel;
mod post;
mod preview;
mod prediction;
mod reconnect;
mod reflection;
mod shake;
mod sunshadow;
mod render_storage;
mod texture;
mod view;
mod voxels;

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
