use std::process::Command;

fn main() {
    let status = Command::new("uniffi-bindgen") // flagged: shells out from build.rs
        .args(["scaffolding", "src/notes.udl", "--out-dir", "src"]) // flagged: src/
        .status()
        .unwrap(); // flagged: no message if the tool is missing
    assert!(status.success());
}
