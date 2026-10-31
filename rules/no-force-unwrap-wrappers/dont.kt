val job = if (initialExperiments != null && isFirstRun) {
    // flagged: UnsafeCallOnNullableType
    applyLocalExperiments(initialExperiments!!)
} else {
    applyPendingExperiments(initial = true)
}
