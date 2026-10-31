/// Parses a comma-separated list of record ids.
fn parse_ids(input: &str) -> (Vec<&str>, usize) {
    let ids: Vec<&str> = input.split(',').map(str::trim).collect();
    let num_received = ids.len();
    (ids, num_received)
}
