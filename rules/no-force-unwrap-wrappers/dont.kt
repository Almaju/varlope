val job = if (initialExperiments != null && isFirstRun) {
    // initialExperiments is a var: the compiler can't prove it is still non-null
    applyLocalExperiments(initialExperiments!!) // flagged: crashes if it became null
} else {
    applyPendingExperiments(initial = true)
}
