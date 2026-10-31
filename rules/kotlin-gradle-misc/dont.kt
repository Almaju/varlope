class KotlinWorkQueue(private val queue: WorkQueue) {
    private val scope = CoroutineScope(Dispatchers.IO) // flagged: InjectDispatcher

    /**
     * Runs the queued items, at most [limit] at a time.
     *
     * @param max the maximum number of items in flight
     */
    fun start(limit: Int) = scope.launch { queue.runAll(limit) } // flagged: stale @param
}
