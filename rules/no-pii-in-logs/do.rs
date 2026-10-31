fn to_outgoing(record: TabsRecord) -> Result<OutgoingBso> {
    trace!("outgoing record {} with {} tabs", record.id, record.tabs.len());
    Ok(OutgoingBso::from_content_with_id(record)?)
}
