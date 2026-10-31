#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord { // flagged: no #[serde(flatten)] unknown_fields
    pub id: SyncGuid,
    #[serde(default)]
    pub title: String, // flagged: `"title": null` from old clients fails to parse
    pub hist_uri: String,
    pub visits: Vec<HistoryRecordVisit>,
}
