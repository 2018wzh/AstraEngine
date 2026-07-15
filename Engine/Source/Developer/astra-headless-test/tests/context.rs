#[astra_headless_test::test]
fn starts_and_stops_checkout_bound_session() {
    assert!(std::env::var_os("ASTRA_BUILD_IDENTITY").is_some());
}

#[astra_headless_test::tokio_test]
async fn async_test_uses_same_per_binary_server() {
    tokio::task::yield_now().await;
}
