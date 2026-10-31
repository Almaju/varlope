pub struct Participation {
    pub experiments: bool,
    pub rollouts: bool,
}

/// Opts the user in or out of experiments and rollouts.
pub fn set_participation(&self, participation: Participation) {
    // …
}

client.set_participation(Participation { experiments: true, rollouts: false });
