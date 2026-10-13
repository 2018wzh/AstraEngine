use super::*;

#[tokio::test]
pub(super) async fn https_package_source_uses_strict_verified_byte_ranges() {
    static PACKAGE: &[u8] = b"deterministic TLS package range fixture";
    let (url, certificate, server) = spawn_tls_range_server(PACKAGE).await;
    let temp = tempfile::tempdir().unwrap();
    let origin = url::Url::parse(&url)
        .unwrap()
        .origin()
        .ascii_serialization();
    let mut profile = HeadlessHostProfile::reference(
        "headless-https-range",
        "com.example.https-range",
        hash(b"build"),
        hash(PACKAGE),
    );
    profile
        .package_sources
        .push(PackageSourcePolicy::HttpsRange {
            allowed_origins: vec![origin],
        });
    profile.limits.max_package_read_bytes = 8;
    let session = HeadlessPlatformFactory::new(temp.path().join("run"), temp.path())
        .with_https_root_certificate_pem(certificate)
        .start(profile.into())
        .await
        .unwrap();
    let source = session
        .client
        .open_package(PackageSourceRequest::HttpsRange {
            url,
            expected_hash: hash(PACKAGE),
        })
        .await
        .unwrap();
    assert_eq!(
        session
            .client
            .read_package_range(source, 14, 8)
            .await
            .unwrap(),
        b"TLS pack"
    );
    session.client.close_package(source).await.unwrap();
    session.client.shutdown().await.unwrap();
    server.abort();
}
