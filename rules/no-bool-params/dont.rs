/// Opts the user in or out of experiments and rollouts.
pub fn set_participation(&self, experiments: bool, rollouts: bool) { // flagged
    // …
}

// call site: which flag is which?
client.set_participation(true, false);
