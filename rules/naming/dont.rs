fn validate_feature(&self, feature_def: &FeatureDef) -> Result<()> {
    for prop in feature_def.props() {
        let nm = prop.name(); // flagged: "`nm` is not at all clear"
        self.validate_prop(&nm, &prop.typ())?;
    }
    let ta = self.targeting_attributes(); // flagged: "not a readable variable name"
    self.check_targeting(&ta)
}
