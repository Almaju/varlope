/// Parses a comma-separated list of record ids.
fn parse_ids(input: &str) -> Vec<&str> {
    let received = input.split(',');
    received.map(str::trim).collect()
}
