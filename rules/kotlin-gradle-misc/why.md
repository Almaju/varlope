Android reviewers need to run component work on their own `CoroutineContext` (exception handlers,
supervisor jobs), so a hard-coded `Dispatchers.IO` should only ever be a parameter default. KDoc
`@param` names drift from the real parameters (`@param start` on `deleteVisitsSince(since)`), eager
Gradle task APIs such as `tasks[name]` break configuration caching, and a new lint or detekt baseline
entry hides a finding instead of fixing it.
