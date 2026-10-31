class FxNimbus: FeatureManifestInterface { // flagged: implicit internal
    static let shared = FxNimbus() // flagged: implicit internal
    var api: FeaturesInterface? // flagged: implicit internal

    func initialize(with getSdk: @escaping () -> FeaturesInterface?) { // flagged
        // …
    }
}
