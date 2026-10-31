public class FxNimbus: FeatureManifestInterface {
    public static let shared = FxNimbus()
    public var api: FeaturesInterface?

    public func initialize(with getSdk: @escaping () -> FeaturesInterface?) {
        // …
    }
}
