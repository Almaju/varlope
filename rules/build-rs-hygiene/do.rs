fn main() {
    // glean_build writes the metrics into OUT_DIR, where lib.rs includes them.
    glean_build::Builder::default()
        .file("metrics.yaml")
        .generate()
        .expect("Error generating Glean Rust bindings");
    let nss = std::env::var("NSS_DIR")
        .expect("NSS_DIR must point at an NSS build: run libs/verify-desktop-environment.sh");
    println!("cargo:rustc-link-search={nss}/lib");
}
