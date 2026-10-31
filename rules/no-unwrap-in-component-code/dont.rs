impl TabsStorage {
    fn remote_clients(&self) -> Result<HashMap<String, RemoteClient>> {
        Ok(match self.get_meta::<String>(REMOTE_CLIENTS_KEY)? {
            None => HashMap::default(),
            Some(json) => serde_json::from_str(&json).unwrap(), // flagged: corrupt meta crashes the app
        })
    }
}
