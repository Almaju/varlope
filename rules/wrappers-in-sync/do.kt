// uses the generated `DeviceType` directly

class FxaClient(private val inner: FirefoxAccount) {
    fun getDevices(ignoreCache: Boolean = false): Array<Device> =
        inner.getDevices(ignoreCache).toTypedArray()
}
