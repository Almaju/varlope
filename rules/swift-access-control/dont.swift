public class FxNimbus: FeatureManifestInterface {
    static let shared = FxNimbus() // flagged: implicitly internal, the app can't reach it
    public var api: FeaturesInterface?

    func initialize(with getSdk: @escaping () -> FeaturesInterface?) { // flagged
        self.getSdk = getSdk
    }
}

extension FxNimbus { // flagged: every member defaults to internal
    func invalidateCachedValues() { /* … */ }
}
