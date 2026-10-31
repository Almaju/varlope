fn export_wrapped_key(key: &SymKey) -> Result<Vec<u8>> {
    let mut item = nss_sys::SECItem { type_: 0, data: ptr::null_mut(), len: 0 };
    map_nss_secstatus(|| unsafe { wrap_key(key, &mut item) })?;
    let bytes = unsafe { sec_item_as_slice(&mut item)? }.to_vec(); // flagged: no SAFETY
    unsafe { nss_sys::SECITEM_FreeItem(&mut item, nss_sys::PR_FALSE) }; // flagged: skipped by `?`
    Ok(bytes)
}
