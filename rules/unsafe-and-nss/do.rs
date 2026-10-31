fn export_wrapped_key(key: &SymKey) -> Result<Vec<u8>> {
    let mut item = ScopedSECItem::empty(nss_sys::SECItemType::siBuffer);
    // SAFETY: `item` is a valid empty SECItem; NSS fills it and `item` owns the data.
    map_nss_secstatus(|| unsafe { wrap_key(key, item.as_mut_ref()) })?;
    // SAFETY: on success `item.data` points to `item.len` bytes owned by `item`.
    let bytes = unsafe { sec_item_as_slice(item.as_mut_ref())? }.to_vec();
    Ok(bytes) // ScopedSECItem's Drop frees the data on every path
}
