enum class FxaDeviceType { DESKTOP, MOBILE, TABLET } // flagged: mirrors the generated `DeviceType`

class FxaClient(private val inner: FirefoxAccount) {
    fun getDevices(): Array<Device> =
        inner.getDevices(false).toTypedArray() // flagged: Rust exposes `ignore_cache`
}
