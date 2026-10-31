// Uses the generated `DeviceType` directly instead of a copy.

class FxaClient(private val inner: FirefoxAccount) {
    fun getDevices(ignoreCache: Boolean = false): Array<Device> =
        inner.getDevices(ignoreCache).toTypedArray()
}
