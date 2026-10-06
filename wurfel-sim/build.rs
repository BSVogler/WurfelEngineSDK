//! Records the git commit this build was made from (`WURFEL_GIT_HASH`, empty outside a git checkout),
//! so the server and the browser client can tell whether they come from the same version.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // A new commit moves HEAD (or the branch it names): build again then.
    if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git(&["rev-parse", "--git-path", &branch]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let hash = git(&["rev-parse", "--short=8", "HEAD"]).unwrap_or_default();
    println!("cargo:rustc-env=WURFEL_GIT_HASH={hash}");
}
