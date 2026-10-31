pub struct Participation {
    pub experiments: bool,
    pub rollouts: bool,
}

impl NimbusClient {
    /// Opts the user in or out of experiments and rollouts.
    pub fn set_participation(&self, participation: Participation) {
        // …
    }
}

fn on_settings_changed(client: &NimbusClient) {
    client.set_participation(Participation { experiments: true, rollouts: false });
}
