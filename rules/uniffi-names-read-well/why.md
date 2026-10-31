Kotlin and Swift developers only see the name after UniFFI camel-cases it, so it has to say what
the call does: a `verify_*` that deletes logins invites someone to call it "to check", a
`run_blocking` means the opposite of Kotlin's `runBlocking`, `Rust` in a type name tells callers
nothing, and `mimetype` next to `mime_type` reads like a typo. Renaming later is a breaking change.
