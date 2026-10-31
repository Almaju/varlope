fn enroll(&self, slug: String) -> Result<Enrollment> {
    let store = self.store.clone(); // flagged: clone_on_ref_ptr
    let enrollment = Enrollment::new(slug.clone(), store); // flagged: redundant_clone
    enrollment.save("manual".to_string())?; // flagged: str_to_string
    Ok(enrollment)
}
