fn validate_feature(&self, feature_def: &FeatureDef) -> Result<()> {
    for prop in feature_def.props() {
        let name = prop.name();
        self.validate_prop(&name, &prop.typ())?;
    }
    let targeting_attributes = self.targeting_attributes();
    self.check_targeting(&targeting_attributes)
}
