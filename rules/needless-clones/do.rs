fn enroll(&self, slug: String) -> Result<Enrollment> {
    let store = Arc::clone(&self.store);
    let enrollment = Enrollment::new(slug, store);
    enrollment.save("manual".to_owned())?;
    Ok(enrollment)
}
