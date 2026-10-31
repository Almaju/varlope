If a foreign (Kotlin/Swift/JS) implementation of a callback throws and the Rust signature has no error type, UniFFI panics in Rust. Reviewer made this the only blocker on the context_id component.
