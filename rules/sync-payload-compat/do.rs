#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
    pub id: SyncGuid,
    #[serde(default, deserialize_with = "deserialize_nonull_string")]
    pub title: String,
    pub hist_uri: String,
    pub visits: Vec<HistoryRecordVisit>,
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}
