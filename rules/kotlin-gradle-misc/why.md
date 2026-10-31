The async-bridge ADR's KotlinWorkQueue hardcoded `Dispatchers.IO`; Android reviewers need to inject their own CoroutineContext (exception handlers, supervisor jobs, non-IO long-running tasks).
