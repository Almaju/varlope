fn main() {
    // uniffi is a build-dependency; this writes notes.uniffi.rs into OUT_DIR,
    // which lib.rs pulls in with `uniffi::include_scaffolding!("notes")`.
    uniffi::generate_scaffolding("src/notes.udl")
        .expect("failed to generate UniFFI scaffolding from src/notes.udl");
}
