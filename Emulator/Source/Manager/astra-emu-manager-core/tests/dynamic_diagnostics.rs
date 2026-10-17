
#[test]
#[ignore = "requires an explicitly built Family plugin binary"]
fn dynamic_library_layout_matches_manager() {
    use astra_emu_abi as abi;
    let path = std::env::var_os("ASTRA_EMU_TEST_PLUGIN").expect("explicit native plugin binary required");
    let library = std::mem::ManuallyDrop::new(unsafe { libloading::Library::new(&path) }.expect("load native library"));
    let query = unsafe { library.get::<abi::Query>(abi::QUERY_SYMBOL) }.expect("native query symbol");
    let mut api = std::ptr::null();
    assert_eq!(unsafe { query(abi::ABI_VERSION, &mut api) }, abi::OK);
    assert!(!api.is_null());
    unsafe { &*api }.validate().unwrap();
    assert_eq!(unsafe { query(u32::MAX, &mut api) }, abi::UNSUPPORTED);
    assert!(api.is_null());
}

