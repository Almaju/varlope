val experiments = initialExperiments
val job = if (experiments != null && isFirstRun) {
    applyLocalExperiments(experiments) // smart cast on a local val
} else {
    applyPendingExperiments(initial = true)
}
