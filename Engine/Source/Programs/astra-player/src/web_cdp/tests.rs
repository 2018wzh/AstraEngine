use super::browser_contract::BundleHttpServer;
use super::*;

fn request(address: std::net::SocketAddr, target: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(address).expect("connect bundle server");
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .expect("write request");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("read response");
    response
}

#[test]
fn bundle_server_serves_entrypoint_with_required_isolation_headers() {
    let root = tempfile::tempdir().expect("bundle root");
    fs::write(root.path().join("index.html"), b"astra").expect("write entrypoint");
    let server = BundleHttpServer::start(root.path()).expect("start server");
    let response = request(server.address(), "/");
    let response = String::from_utf8(response).expect("utf8 response");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("Content-Type: text/html; charset=utf-8\r\n"));
    assert!(response.contains("Cross-Origin-Opener-Policy: same-origin\r\n"));
    assert!(response.contains("Cross-Origin-Embedder-Policy: require-corp\r\n"));
    assert!(response.ends_with("\r\n\r\nastra"));
}

#[test]
fn bundle_server_rejects_encoded_and_parent_traversal() {
    let root = tempfile::tempdir().expect("bundle root");
    fs::write(root.path().join("index.html"), b"astra").expect("write entrypoint");
    let server = BundleHttpServer::start(root.path()).expect("start server");
    for target in ["/%2e%2e/secret", "/../secret", "/C:/secret"] {
        let response = request(server.address(), target);
        assert!(
            response.starts_with(b"HTTP/1.1 400 Bad Request\r\n"),
            "target {target} was not rejected"
        );
    }
}
