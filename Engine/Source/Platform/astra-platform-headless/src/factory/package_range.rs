use super::*;

pub(super) async fn package_range(
    source: &mut PackageState,
    offset: u64,
    length: usize,
) -> Result<Vec<u8>, PlatformError> {
    match source {
        PackageState::File(file) => file.read_range(offset, length),
        PackageState::HttpsRange {
            client,
            url,
            length: package_length,
            block_size,
            block_hashes,
        } => {
            if length == 0 {
                return Err(invalid(
                    "package.read_range",
                    "range length must be non-zero",
                ));
            }
            if offset >= *package_length {
                return Err(invalid(
                    "package.read_range",
                    "range starts outside package",
                ));
            }
            let requested_end = offset
                .checked_add(length as u64)
                .ok_or_else(|| invalid("package.read_range", "range overflows"))?
                .min(*package_length);
            let first_block = offset / *block_size as u64;
            let last_block = (requested_end - 1) / *block_size as u64;
            let mut verified = Vec::new();
            for block_index in first_block..=last_block {
                let block_start = block_index * *block_size as u64;
                let block_end = (block_start + *block_size as u64).min(*package_length) - 1;
                let (bytes, _) =
                    fetch_https_range(client, url, block_start, block_end, *package_length).await?;
                let actual: [u8; 32] = Sha256::digest(&bytes).into();
                let expected = block_hashes.get(block_index as usize).ok_or_else(|| {
                    invalid("package.read_range", "HTTPS block identity is missing")
                })?;
                if &actual != expected {
                    return Err(PlatformError::new(
                        PlatformErrorCode::IntegrityMismatch,
                        "package.read_range",
                        "HTTPS package block hash mismatch",
                    ));
                }
                verified.extend_from_slice(&bytes);
            }
            let relative_start = usize::try_from(offset - first_block * *block_size as u64)
                .map_err(|_| invalid("package.read_range", "range offset overflows"))?;
            let requested_length = usize::try_from(requested_end - offset)
                .map_err(|_| invalid("package.read_range", "range length overflows"))?;
            Ok(verified[relative_start..relative_start + requested_length].to_vec())
        }
    }
}

pub(super) async fn open_https_range(
    url: url::Url,
    expected_hash: &str,
    max_package_bytes: u64,
    max_read_bytes: usize,
    root_certificates: &[Vec<u8>],
) -> Result<PackageState, PlatformError> {
    if max_read_bytes == 0 {
        return Err(invalid(
            "package.open_https",
            "HTTPS range block size must be non-zero",
        ));
    }
    let mut client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
    for certificate in root_certificates {
        let certificate = reqwest::Certificate::from_pem(certificate)
            .map_err(|_| invalid("package.open_https", "HTTPS root certificate is invalid"))?;
        client = client.add_root_certificate(certificate);
    }
    let client = client.build().map_err(|_| io_error("package.open_https"))?;
    let (_, length) = fetch_https_range(&client, &url, 0, 0, 0).await?;
    if length == 0 || length > max_package_bytes {
        return Err(invalid(
            "package.open_https",
            "HTTPS package length is empty or exceeds the declared package byte limit",
        ));
    }

    let block_size = max_read_bytes.min(1024 * 1024);
    let mut package_hasher = Sha256::new();
    let mut block_hashes = Vec::new();
    let mut start = 0_u64;
    while start < length {
        let end = (start + block_size as u64).min(length) - 1;
        let (bytes, _) = fetch_https_range(&client, &url, start, end, length).await?;
        package_hasher.update(&bytes);
        block_hashes.push(Sha256::digest(&bytes).into());
        start = end + 1;
    }
    let actual = format!("sha256:{:x}", package_hasher.finalize());
    if actual != expected_hash {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "package.open_https",
            "HTTPS package hash mismatch",
        ));
    }
    Ok(PackageState::HttpsRange {
        client,
        url,
        length,
        block_size,
        block_hashes,
    })
}

pub(super) async fn fetch_https_range(
    client: &reqwest::Client,
    url: &url::Url,
    start: u64,
    end: u64,
    expected_length: u64,
) -> Result<(Vec<u8>, u64), PlatformError> {
    let response = client
        .get(url.clone())
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, format!("bytes={start}-{end}"))
        .send()
        .await
        .map_err(|_| io_error("package.read_https_range"))?;
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(invalid(
            "package.read_https_range",
            "HTTPS package server must honor byte range requests without redirects",
        ));
    }
    let content_range = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| invalid("package.read_https_range", "Content-Range is missing"))?;
    let (actual_start, actual_end, total) = parse_content_range(content_range)?;
    if actual_start != start
        || actual_end != end
        || (expected_length != 0 && total != expected_length)
    {
        return Err(invalid(
            "package.read_https_range",
            "HTTPS Content-Range does not match the requested package range",
        ));
    }
    let expected_bytes = usize::try_from(end - start + 1)
        .map_err(|_| invalid("package.read_https_range", "range length overflows"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|_| io_error("package.read_https_range"))?;
    if bytes.len() != expected_bytes {
        return Err(invalid(
            "package.read_https_range",
            "HTTPS range response byte length is invalid",
        ));
    }
    Ok((bytes.to_vec(), total))
}

pub(super) fn parse_content_range(value: &str) -> Result<(u64, u64, u64), PlatformError> {
    let value = value.strip_prefix("bytes ").ok_or_else(|| {
        invalid(
            "package.read_https_range",
            "Content-Range unit must be bytes",
        )
    })?;
    let (range, total) = value
        .split_once('/')
        .ok_or_else(|| invalid("package.read_https_range", "Content-Range shape is invalid"))?;
    let (start, end) = range.split_once('-').ok_or_else(|| {
        invalid(
            "package.read_https_range",
            "Content-Range bounds are invalid",
        )
    })?;
    let start = start
        .parse::<u64>()
        .map_err(|_| invalid("package.read_https_range", "Content-Range start is invalid"))?;
    let end = end
        .parse::<u64>()
        .map_err(|_| invalid("package.read_https_range", "Content-Range end is invalid"))?;
    let total = total
        .parse::<u64>()
        .map_err(|_| invalid("package.read_https_range", "Content-Range total is invalid"))?;
    if start > end || end >= total {
        return Err(invalid(
            "package.read_https_range",
            "Content-Range bounds exceed the package length",
        ));
    }
    Ok((start, end, total))
}

pub(super) fn complete_gpu_sample(
    mut sample: HeadlessGpuFrameSample,
    submission: WgpuProfiledSubmission,
) -> HeadlessGpuFrameSample {
    sample.cpu_submit_ns = submission.cpu_submit_ns;
    sample.gpu_duration_ns = submission.gpu_duration_ns;
    sample.scene_cpu_ns = submission.scene_cpu_ns;
    sample.filter_cpu_ns = submission.filter_cpu_ns;
    sample.scene_command_cpu_ns = submission.scene_command_cpu_ns;
    sample.scene_atlas_cpu_ns = submission.scene_atlas_cpu_ns;
    sample.scene_geometry_cpu_ns = submission.scene_geometry_cpu_ns;
    sample.scene_vertex_upload_cpu_ns = submission.scene_vertex_upload_cpu_ns;
    sample.scene_render_encode_cpu_ns = submission.scene_render_encode_cpu_ns;
    sample.scene_queue_submit_cpu_ns = submission.scene_queue_submit_cpu_ns;
    sample.scene_render_submit_cpu_ns = submission.scene_render_submit_cpu_ns;
    sample.atlas_upload_gpu_ns = submission.atlas_upload_gpu_ns;
    sample.scene_gpu_ns = submission.scene_gpu_ns;
    sample.filter_gpu_ns = submission.filter_gpu_ns;
    sample
}

pub(super) fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, PlatformError> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(PlatformError::new(
            PlatformErrorCode::PermissionDenied,
            "package.open",
            "bundled package path is not a safe relative path",
        ));
    }
    Ok(root.join(path))
}

pub(super) struct Sha256Writer<'a>(pub(super) &'a mut Sha256);

impl Write for Sha256Writer<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn canonical_json_digest(
    value: &impl serde::Serialize,
) -> Result<[u8; 32], serde_json::Error> {
    let mut digest = Sha256::new();
    serde_json::to_writer(Sha256Writer(&mut digest), value)?;
    Ok(digest.finalize().into())
}

pub(super) fn scene_submission_identity(
    sequence: u64,
    width: u32,
    height: u32,
    clear_rgba: [u8; 4],
    command_count: usize,
    semantics: &Option<astra_ui_core::UiSemanticSnapshot>,
) -> Result<[u8; 32], PlatformError> {
    let command_count = u32::try_from(command_count).map_err(|_| {
        invalid(
            "surface.present_scene",
            "scene command count exceeds the submission identity format",
        )
    })?;
    let mut identity = [0_u8; 32];
    identity[..8].copy_from_slice(&sequence.to_le_bytes());
    identity[8..12].copy_from_slice(&width.to_le_bytes());
    identity[12..16].copy_from_slice(&height.to_le_bytes());
    identity[16..20].copy_from_slice(&command_count.to_le_bytes());
    identity[20..24].copy_from_slice(&clear_rgba);
    identity[24..32].copy_from_slice(
        &semantics
            .as_ref()
            .map_or(0, |snapshot| snapshot.generation)
            .to_le_bytes(),
    );
    Ok(identity)
}

pub(super) fn elapsed_ns(
    started: Instant,
    operation: &'static str,
    overflow_message: &'static str,
) -> Result<u64, PlatformError> {
    started
        .elapsed()
        .as_nanos()
        .try_into()
        .map_err(|_| invalid(operation, overflow_message))
}

pub(super) fn invalid(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::InvalidState, operation, message)
}

pub(super) fn io_error(operation: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::Io,
        operation,
        "headless I/O operation failed",
    )
}

pub(super) fn media_error(error: astra_media::MediaError) -> PlatformError {
    let diagnostic = error.to_string();
    let diagnostic_codes = match &error {
        MediaError::Diagnostics(diagnostics) => diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>()
            .join(","),
        MediaError::Message(_) => "ASTRA_MEDIA_PROVIDER_MESSAGE".to_owned(),
    };
    PlatformError::new(
        PlatformErrorCode::IntegrityMismatch,
        "headless.media",
        format!("headless media provider rejected input: {diagnostic}"),
    )
    .with_field("media_error", diagnostic)
    .with_field("diagnostic_codes", diagnostic_codes)
}
