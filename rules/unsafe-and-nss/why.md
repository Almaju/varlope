Test helpers in nimbus used `unsafe { std::mem::transmute::<&Box<dyn MetricsHandler>, &Box<TestMetrics>>(..) }`; reviewers found one was outright wrong and asked for SAFETY comments on the rest.
