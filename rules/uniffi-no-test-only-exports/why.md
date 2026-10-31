Everything exported through UniFFI ships in the Kotlin, Swift and JS API of every app, where a
test helper can be called in production and must be kept stable like any other API. Reviewers keep
asking whether a helper "only used for testing" can be tested another way, and helpers such as
`set_last_sync` or `open_in_memory` belong in a non-exported `#[cfg(any(test, feature = "testing"))]`
block that the megazords never enable.
