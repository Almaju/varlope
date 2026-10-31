class KotlinWorkQueue(private val queue: WorkQueue) {
    private val scope = CoroutineScope(Dispatchers.IO) // flagged: apps can't inject a context

    /**
     * Runs the queued items, at most [limit] at a time.
     *
     * @param max the maximum number of items in flight (flagged: no such parameter)
     */
    fun start(limit: Int) = scope.launch { queue.runAll(limit) }
}
