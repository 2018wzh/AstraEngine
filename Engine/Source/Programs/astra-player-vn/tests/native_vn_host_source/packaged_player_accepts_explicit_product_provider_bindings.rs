use super::*;

#[test]
pub(super) fn packaged_player_accepts_explicit_product_provider_bindings() {
    let bytes = product_package();
    let package = astra_package::PackageReader::open(&bytes).unwrap();

    NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .expect("product-bound package should open");
}
