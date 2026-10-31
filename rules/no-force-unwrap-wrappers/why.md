Kotlin `!!` and Swift `!`, `as!` and `try!` turn an unexpected null or error into an app crash, and
these wrappers (plus the Swift and Kotlin that nimbus-fml generates) run inside Firefox for Android and iOS.
Reviewers ask for a local `val`, `?.let`/`?:` or `guard let`, or an explicit `requireNotNull(x) { "why" }`
when crashing really is intended, and rejected the opposite extreme of quietly returning `""` instead.
