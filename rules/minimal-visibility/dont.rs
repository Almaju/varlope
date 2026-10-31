// src/lib.rs
mod schema;

// src/schema.rs
pub const VERSION: u32 = 7; // flagged: unreachable_pub

pub struct StoreConnectionInitializer; // flagged: unreachable_pub

impl ConnectionInitializer for StoreConnectionInitializer {
    const NAME: &'static str = "store db";
    const END_VERSION: u32 = VERSION;
    // …
}
