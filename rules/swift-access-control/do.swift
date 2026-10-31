public class FxNimbus: FeatureManifestInterface {
    public static let shared = FxNimbus()
    public var api: FeaturesInterface?
    private var getSdk: GetSdk?

    public func initialize(with getSdk: @escaping () -> FeaturesInterface?) {
        self.getSdk = getSdk
    }
}

public extension FxNimbus {
    func invalidateCachedValues() { /* … */ }
}
