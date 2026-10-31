class KotlinWorkQueue(
    private val queue: WorkQueue,
    coroutineContext: CoroutineContext = Dispatchers.IO,
) {
    private val scope = CoroutineScope(coroutineContext)

    /**
     * Runs the queued items, at most [limit] at a time.
     *
     * @param limit the maximum number of items in flight
     */
    fun start(limit: Int) = scope.launch { queue.runAll(limit) }
}
