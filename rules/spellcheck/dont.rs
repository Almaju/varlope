/// Parses a comma-seperated list of record ids. // flagged: seperated
fn parse_ids(input: &str) -> (Vec<&str>, usize) {
    let ids: Vec<&str> = input.split(',').map(str::trim).collect();
    let num_recieved = ids.len(); // flagged: recieved
    (ids, num_recieved)
}
