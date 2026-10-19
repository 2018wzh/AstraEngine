use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::Mutex;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::index::{IndexedFile, METADATA_INDEX_SCHEMA_VERSION};

const ETAG_VALUE: &str = "\"metadata-1\"";
const DATA_FILES: [(&str, &[u8]); 3] = [
    (
        "compatibility.json",
        br#"{"schema":"astra.emu.compatibility.v3","generated_at_unix_ms":1,"entries":[]}"#,
    ),
    (
        "presets.json",
        br#"{"schema":"astra.emu.presets.v1","entries":[]}"#,
    ),
    (
        "cores.json",
        br#"{"schema":"astra.emu.cores.v1","entries":[]}"#,
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    path: String,
    if_none_match: Option<String>,
}

struct Fixture {
    base_url: String,
    certificate_pem: Vec<u8>,
    requests: Arc<Mutex<Vec<Request>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Fixture {
    fn client(&self) -> MetadataSyncClient {
        let certificate = reqwest::Certificate::from_pem(&self.certificate_pem).unwrap();
        MetadataSyncClient::with_builder(
            &self.base_url,
            reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .no_proxy()
                .tls_certs_only([certificate]),
        )
        .unwrap()
    }

    fn take_requests(&self) -> Vec<Request> {
        std::mem::take(&mut *self.requests.lock())
    }
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn index_bytes() -> Vec<u8> {
    let index = MetadataIndex {
        schema: METADATA_INDEX_SCHEMA_VERSION.into(),
        generated_at_unix_ms: 1_700_000_000_000,
        source_commit: "0123456789abcdef0123456789abcdef01234567".into(),
        files: DATA_FILES
            .iter()
            .map(|(path, bytes)| IndexedFile {
                path: (*path).into(),
                sha256: sha256(bytes),
                size: bytes.len() as u64,
            })
            .collect(),
    };
    serde_json::to_vec(&index).unwrap()
}

async fn spawn_server(routes: BTreeMap<String, Vec<u8>>) -> Fixture {
    if tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
    }
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let certificate_pem = certified.cert.pem().into_bytes();
    let key = tokio_rustls::rustls::pki_types::PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    );
    let config = tokio_rustls::rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certified.cert.der().clone()], key.into())
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let routes = Arc::new(routes);
    let log = requests.clone();
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            let routes = routes.clone();
            let log = log.clone();
            tokio::spawn(async move {
                let Ok(mut stream) = acceptor.accept(stream).await else {
                    return;
                };
                let mut head = Vec::new();
                while !head.windows(4).any(|window| window == b"\r\n\r\n") {
                    let mut chunk = [0_u8; 1024];
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(count) => head.extend_from_slice(&chunk[..count]),
                    }
                    assert!(head.len() <= 16 * 1024);
                }
                let head = String::from_utf8(head).unwrap();
                let mut lines = head.lines();
                let path = lines
                    .next()
                    .and_then(|line| line.split(' ').nth(1))
                    .unwrap()
                    .to_owned();
                let if_none_match = lines.find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("if-none-match")
                        .then(|| value.trim().to_owned())
                });
                log.lock().push(Request {
                    path: path.clone(),
                    if_none_match: if_none_match.clone(),
                });
                let (status, body): (&str, &[u8]) =
                    if path == "/v1/index.json" && if_none_match.as_deref() == Some(ETAG_VALUE) {
                        ("304 Not Modified", b"")
                    } else if let Some(body) = routes.get(&path) {
                        ("200 OK", body)
                    } else {
                        ("404 Not Found", b"")
                    };
                let header = format!(
                    "HTTP/1.1 {status}\r\nETag: {ETAG_VALUE}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if stream.write_all(header.as_bytes()).await.is_ok()
                    && stream.write_all(body).await.is_ok()
                {
                    let _ = stream.shutdown().await;
                }
            });
        }
    });
    Fixture {
        base_url: format!("https://localhost:{port}/v1/"),
        certificate_pem,
        requests,
        task,
    }
}

fn routes(tamper: Option<&str>) -> BTreeMap<String, Vec<u8>> {
    let mut routes = BTreeMap::from([("/v1/index.json".to_owned(), index_bytes())]);
    for (path, bytes) in DATA_FILES {
        let mut body = bytes.to_vec();
        if tamper == Some(path) {
            body[0] = b' ';
        }
        routes.insert(format!("/v1/{path}"), body);
    }
    routes
}

fn paths(requests: &[Request]) -> Vec<&str> {
    requests
        .iter()
        .map(|request| request.path.as_str())
        .collect()
}

#[tokio::test]
async fn first_sync_downloads_index_and_verified_files_then_etag_yields_unchanged() {
    let fixture = spawn_server(routes(None)).await;
    let client = fixture.client();

    let outcome = client.sync(None, &BTreeMap::new()).await.unwrap();
    let SyncOutcome::Updated { index, etag, files } = outcome else {
        panic!("expected Updated");
    };
    assert_eq!(index.files.len(), 3);
    assert_eq!(etag.as_deref(), Some(ETAG_VALUE));
    let expected: Vec<(String, Vec<u8>)> = DATA_FILES
        .iter()
        .map(|(path, bytes)| ((*path).to_owned(), bytes.to_vec()))
        .collect();
    assert_eq!(files, expected);
    assert_eq!(
        paths(&fixture.take_requests()),
        [
            "/v1/index.json",
            "/v1/compatibility.json",
            "/v1/presets.json",
            "/v1/cores.json"
        ]
    );

    let cached: BTreeMap<String, String> = index
        .files
        .iter()
        .map(|file| (file.path.clone(), file.sha256.clone()))
        .collect();
    let outcome = client.sync(etag.as_deref(), &cached).await.unwrap();
    assert_eq!(outcome, SyncOutcome::Unchanged);
    assert_eq!(
        fixture.take_requests(),
        [Request {
            path: "/v1/index.json".into(),
            if_none_match: Some(ETAG_VALUE.into()),
        }]
    );
}

#[tokio::test]
async fn changed_index_downloads_only_files_whose_hash_differs() {
    let fixture = spawn_server(routes(None)).await;
    let cached = BTreeMap::from([
        ("compatibility.json".to_owned(), sha256(DATA_FILES[0].1)),
        ("presets.json".to_owned(), "0".repeat(64)),
        ("cores.json".to_owned(), sha256(DATA_FILES[2].1)),
    ]);
    let outcome = fixture
        .client()
        .sync(Some("\"stale\""), &cached)
        .await
        .unwrap();
    let SyncOutcome::Updated { files, .. } = outcome else {
        panic!("expected Updated");
    };
    assert_eq!(
        files,
        [("presets.json".to_owned(), DATA_FILES[1].1.to_vec())]
    );
    assert_eq!(
        paths(&fixture.take_requests()),
        ["/v1/index.json", "/v1/presets.json"]
    );
}

#[tokio::test]
async fn tampered_file_is_rejected_with_hash_mismatch() {
    let fixture = spawn_server(routes(Some("presets.json"))).await;
    let error = fixture
        .client()
        .sync(None, &BTreeMap::new())
        .await
        .unwrap_err();
    assert!(matches!(&error, MetadataError::HashMismatch(path) if path == "presets.json"));
    assert!(error
        .to_string()
        .starts_with("ASTRA_EMU_METADATA_HASH_MISMATCH"));
}

#[tokio::test]
async fn oversized_file_is_rejected_with_hash_mismatch() {
    let mut routes = routes(None);
    routes
        .get_mut("/v1/cores.json")
        .unwrap()
        .extend_from_slice(b"     ");
    let fixture = spawn_server(routes).await;
    let error = fixture
        .client()
        .sync(None, &BTreeMap::new())
        .await
        .unwrap_err();
    assert!(matches!(&error, MetadataError::HashMismatch(path) if path == "cores.json"));
}

#[tokio::test]
async fn missing_index_is_a_network_error() {
    let fixture = spawn_server(BTreeMap::new()).await;
    let error = fixture
        .client()
        .sync(None, &BTreeMap::new())
        .await
        .unwrap_err();
    assert!(matches!(&error, MetadataError::Network(detail) if detail == "http-404"));
}

#[test]
fn base_url_must_be_https_directory_without_credentials() {
    let timeout = Duration::from_secs(3);
    for url in [
        "http://2018wzh.github.io/AstraEMU-metadata/v1/",
        "https://2018wzh.github.io/AstraEMU-metadata/v1",
        "https://user:pass@2018wzh.github.io/v1/",
        "https://2018wzh.github.io/v1/?x=1",
        "https://2018wzh.github.io/v1/#frag",
        "not a url",
    ] {
        assert!(
            matches!(
                MetadataSyncClient::new(url, timeout),
                Err(MetadataError::InvalidRequest("base_url"))
            ),
            "{url}"
        );
    }
    let client = MetadataSyncClient::new(DEFAULT_METADATA_BASE_URL, timeout).unwrap();
    assert_eq!(client.base_url(), DEFAULT_METADATA_BASE_URL);
}
