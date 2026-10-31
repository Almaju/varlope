NimbusClient::new read the RKV event store eagerly (request-changes over possible main-thread blocking), and Kotlin recordEvent called into Rust I/O directly instead of launching on dbScope.
