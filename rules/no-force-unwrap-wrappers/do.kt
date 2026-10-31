val experiments = initialExperiments
val job = if (experiments != null && isFirstRun) {
    applyLocalExperiments(experiments)
} else {
    applyPendingExperiments(initial = true)
}
