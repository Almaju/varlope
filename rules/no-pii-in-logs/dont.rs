fn to_outgoing(record: TabsRecord) -> Result<OutgoingBso> {
    trace!("outgoing record: {record:?}"); // flagged: Debug of a record leaks URLs/titles
    Ok(OutgoingBso::from_content_with_id(record)?)
}
