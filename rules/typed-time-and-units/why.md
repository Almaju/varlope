An integer timeout or TTL leaves every caller guessing the unit: consumers passed `Date.now()` into
fields they weren't sure were milliseconds, and `periodic_interval: u64 // In seconds` relies on a
comment. `std::time::Duration` and `SystemTime` carry their unit and cross UniFFI. Falling back to
"now" when a server time doesn't parse hides corrupt data and skews anything ordered by time; return
an error or skip the record instead.
