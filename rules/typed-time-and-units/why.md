An integer timeout or TTL keeps its unit in a name or a comment, and those drift:
`HISTORY_TTL: u32 = 5_184_000; // 60 days in milliseconds` actually holds seconds, and consumers
passed `Date.now()` into fields they weren't sure were milliseconds. `std::time::Duration` and
`SystemTime` carry their unit and cross UniFFI. Falling back to "now" when a server time doesn't
parse hides corrupt data; return an error or skip the record instead.
