fn index_by_pref(enrollments: &[PrefEnrollment]) -> HashMap<String, Vec<String>> {
    let mut by_pref = HashMap::new();
    for e in enrollments {
        by_pref.entry(e.pref_name.clone()).or_default().push(e.slug.clone());
    }
    by_pref
}
