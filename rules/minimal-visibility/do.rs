// src/lib.rs
mod schema;

// src/schema.rs
pub(crate) const VERSION: u32 = 7;

pub(crate) struct StoreConnectionInitializer;

impl ConnectionInitializer for StoreConnectionInitializer {
    const NAME: &'static str = "store db";
    const END_VERSION: u32 = VERSION;
    // …
}
