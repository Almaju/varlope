// flagged: mirrors the generated `DeviceType` enum; define it once in Rust
enum class FxaDeviceType { DESKTOP, MOBILE, TABLET }

class FxaClient(private val inner: FirefoxAccount) {
    fun getDevices(): Array<Device> =
        // flagged: Rust exposes `ignore_cache`; the wrapper hard-codes it
        inner.getDevices(false).toTypedArray()
}
