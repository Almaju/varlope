When the exported `uniffi::Object` starts owning workers or telemetry, core modules end up importing
`MozAdsClient`, `AdsClientApiResult` and `#[handle_error]` to talk back to it, which creates cycles
and ties the core to one set of bindings. Reviewers on ads-client asked to keep the FFI object to
`Arc`/`Mutex` plumbing and error conversion, and to let the core type own everything else.
