use std::{env, fs, process::Command};

fn main() {
    // Generate the Glean metrics with the Python glean_parser.
    let python = Command::new("python3") // flagged: depends on PATH and platform
        .args(["-m", "glean_parser", "translate", "metrics.yaml", "-f", "rust"])
        .output()
        .expect("Error running glean_parser");
    fs::write("src/metrics.rs", python.stdout).unwrap(); // flagged: writes into src/
    println!("cargo:rustc-link-search={}", env::var("NSS_DIR").unwrap()); // flagged
}
