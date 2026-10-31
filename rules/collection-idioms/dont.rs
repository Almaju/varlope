fn index_by_pref(enrollments: &[PrefEnrollment]) -> HashMap<String, Vec<String>> {
    let mut by_pref = HashMap::new();
    for e in enrollments.iter() { // flagged: explicit_iter_loop
        if by_pref.contains_key(&e.pref_name) { // flagged: map_entry
            by_pref.get_mut(&e.pref_name).unwrap().push(e.slug.clone());
        } else {
            by_pref.insert(e.pref_name.clone(), vec![e.slug.clone()]);
        }
    }
    by_pref
}
