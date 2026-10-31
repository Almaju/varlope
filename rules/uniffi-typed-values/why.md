A string code, a magic integer or a JSON string leaves every Kotlin, Swift and JS caller to know
the valid values and parse them, and a typo only shows up at runtime. Reviewers keep turning such
fields into types: a Yelp subject type passed as `i32` became `SubjectType`, an
`amp_experiment_branch: u32` where "0 means treatment" became an `AmpMatchingStrategy` enum, and a
stored JSON `Value` became an enum so consumers didn't have to deserialize it.
