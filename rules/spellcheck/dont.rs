/// Parses a comma-seperated list of record ids. // flagged: seperated
fn parse_ids(input: &str) -> Vec<&str> {
    let recieved = input.split(','); // flagged: recieved
    recieved.map(str::trim).collect()
}
