use std::{
    env, fs,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use browser_harbor::{
    domain::{
        release::{ArchiveFormat, Artifact, Integrity, ReleaseDescriptor},
        runtime::{ChromePlusRelease, LibportableRelease},
        settings::NetworkSettings,
        target::TargetId,
        version::{VersionRelation, compare_versions},
    },
    providers::ProviderResolver,
    runtime,
    services::{
        http::HttpClient,
        trusted_metadata::{NO_EXPIRY, TrustedMetadata, verify_with_public_key},
    },
};
use ed25519_dalek::{Signer, SigningKey};
use futures_util::StreamExt;
use rand_core::OsRng;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_ENCODING};
use sha2::{Digest, Sha256};

const MAX_SAMPLE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("keygen") => {
            let private_key = required_path(&mut args, "--private-key")?;
            let public_key = required_path(&mut args, "--public-key")?;
            keygen(&private_key, &public_key)
        }
        Some("sign") => {
            let manifest = required_path(&mut args, "--manifest")?;
            let private_key = required_path(&mut args, "--private-key")?;
            let signature = required_path(&mut args, "--signature")?;
            sign(&manifest, &private_key, &signature)
        }
        Some("sign-env") => {
            let manifest = required_path(&mut args, "--manifest")?;
            let variable = required_value(&mut args, "--variable")?;
            let signature = required_path(&mut args, "--signature")?;
            sign_from_env(&manifest, &variable, &signature)
        }
        Some("verify") => {
            let manifest = required_path(&mut args, "--manifest")?;
            let signature = required_path(&mut args, "--signature")?;
            let public_key = required_path(&mut args, "--public-key")?;
            verify(&manifest, &signature, &public_key)
        }
        Some("refresh") => {
            let baseline = required_path(&mut args, "--baseline")?;
            let signature = required_path(&mut args, "--signature")?;
            let public_key = required_path(&mut args, "--public-key")?;
            let output = required_path(&mut args, "--output")?;
            refresh(&baseline, &signature, &public_key, &output).await
        }
        Some("probe") => {
            let baseline = required_path(&mut args, "--baseline")?;
            let signature = required_path(&mut args, "--signature")?;
            let public_key = required_path(&mut args, "--public-key")?;
            probe(&baseline, &signature, &public_key).await
        }
        _ => Err("usage: metadata keygen|sign|sign-env|verify|refresh|probe [--path VALUE]".into()),
    }
}

fn keygen(
    private_key_path: &Path,
    public_key_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if non_empty_file(private_key_path) || non_empty_trimmed_file(public_key_path) {
        return Err("refusing to overwrite an existing signing key".into());
    }
    let key = SigningKey::generate(&mut OsRng);
    write_new(private_key_path, hex::encode(key.to_bytes()).as_bytes())?;
    write_new(
        public_key_path,
        hex::encode(key.verifying_key().to_bytes()).as_bytes(),
    )
}

fn non_empty_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0)
}

fn non_empty_trimmed_file(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|content| !content.trim().is_empty())
}

fn sign(
    manifest_path: &Path,
    private_key_path: &Path,
    signature_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = fs::read(manifest_path)?;
    let private_key = fs::read_to_string(private_key_path)?;
    sign_bytes(&bytes, &private_key, signature_path)
}

fn sign_from_env(
    manifest_path: &Path,
    variable: &str,
    signature_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = fs::read(manifest_path)?;
    let private_key = env::var(variable)?;
    sign_bytes(&bytes, &private_key, signature_path)
}

fn sign_bytes(
    bytes: &[u8],
    private_key: &str,
    signature_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let private_key = hex::decode(private_key.trim())?;
    let private_key: [u8; 32] = private_key
        .try_into()
        .map_err(|_| "private key must contain 32 bytes")?;
    let key = SigningKey::from_bytes(&private_key);
    let signature = hex::encode(key.sign(bytes).to_bytes());
    fs::write(signature_path, format!("{signature}\n"))?;
    Ok(())
}

fn verify(
    manifest_path: &Path,
    signature_path: &Path,
    public_key_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = fs::read(manifest_path)?;
    let signature = fs::read_to_string(signature_path)?;
    let public_key = fs::read_to_string(public_key_path)?;
    verify_with_public_key(&bytes, &signature, &public_key)?;
    println!("trusted metadata signature verified");
    Ok(())
}

fn required_path(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    required_value(args, flag).map(PathBuf::from)
}

fn required_value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let actual = args.next().ok_or_else(|| format!("missing flag {flag}"))?;
    if actual != flag {
        return Err(format!("expected flag {flag}, found {actual}").into());
    }
    args.next()
        .ok_or_else(|| format!("missing value after {flag}").into())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

async fn refresh(
    baseline_path: &Path,
    signature_path: &Path,
    public_key_path: &Path,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let baseline_bytes = fs::read(baseline_path)?;
    let signature = fs::read_to_string(signature_path)?;
    let public_key = fs::read_to_string(public_key_path)?;
    let baseline = verify_with_public_key(&baseline_bytes, &signature, &public_key)?;
    let work = TempDirectory::create()?;
    let network = NetworkSettings {
        accelerators_enabled: false,
        ..NetworkSettings::default()
    };
    let http = HttpClient::new(&network)?;
    let provider = ProviderResolver::new(http.clone());
    let artifact_client = HttpClient::download_client(&network)?;

    let mut releases = Vec::new();
    for target in TargetId::ALL {
        let spec = target.spec();
        for channel in spec.channels {
            for cpu_build in spec.cpu_builds {
                println!(
                    "Refreshing {} / {} / {}",
                    target.display_name(),
                    channel.display_name(),
                    cpu_build.display_name()
                );
                let refreshed = async {
                    let descriptor =
                        resolve_target(&provider, target, *channel, *cpu_build).await?;
                    ensure_release_integrity(descriptor, &baseline, &artifact_client, &work.path)
                        .await
                }
                .await;
                match refreshed {
                    Ok(Some(descriptor)) => releases.push(descriptor),
                    Ok(None) => {}
                    Err(error) => {
                        let reason = safe_refresh_error(error.as_ref());
                        if let Some(previous) = baseline.release(target, *channel, *cpu_build) {
                            if target == TargetId::Edge
                                && previous.artifact.format == ArchiveFormat::Msi
                            {
                                eprintln!(
                                    "{} / {} / {} refresh failed; dropping stale MSI baseline: {reason}",
                                    target.display_name(),
                                    channel.display_name(),
                                    cpu_build.display_name()
                                );
                                continue;
                            }
                            eprintln!(
                                "{} / {} / {} refresh failed; keeping baseline: {reason}",
                                target.display_name(),
                                channel.display_name(),
                                cpu_build.display_name()
                            );
                            releases.push(previous.clone());
                        } else {
                            return Err(reason.into());
                        }
                    }
                }
            }
        }
    }

    println!("Refreshing Chrome++");
    let chrome_plus = refresh_component("Chrome++", baseline.chrome_plus.as_ref(), async {
        ensure_chrome_plus_integrity(
            resolve_chrome_plus(&http).await?,
            &baseline,
            &artifact_client,
            &work.path,
        )
        .await
    })
    .await?;
    println!("Refreshing Libportable");
    let libportable = refresh_component("Libportable", baseline.libportable.as_ref(), async {
        ensure_libportable_integrity(
            resolve_libportable(&http).await?,
            &baseline,
            &artifact_client,
            &work.path,
        )
        .await
    })
    .await?;

    let content_changed = releases != baseline.releases
        || baseline.chrome_plus.as_ref() != Some(&chrome_plus)
        || baseline.libportable.as_ref() != Some(&libportable);
    let now = unix_now();
    let refresh_time = content_changed || baseline.expires_at != NO_EXPIRY;
    let generated_at = if content_changed {
        now
    } else {
        baseline.generated_at
    };
    let expires_at = if refresh_time {
        NO_EXPIRY
    } else {
        baseline.expires_at
    };
    let metadata = TrustedMetadata {
        schema_version: 1,
        generated_at,
        expires_at,
        releases,
        chrome_plus: Some(chrome_plus),
        libportable: Some(libportable),
    };
    let bytes = serde_json::to_vec_pretty(&metadata)?;
    fs::write(output_path, [bytes.as_slice(), b"\n"].concat())?;
    Ok(())
}

async fn probe(
    baseline_path: &Path,
    signature_path: &Path,
    public_key_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let baseline_bytes = fs::read(baseline_path)?;
    let signature = fs::read_to_string(signature_path)?;
    let public_key = fs::read_to_string(public_key_path)?;
    let baseline = verify_with_public_key(&baseline_bytes, &signature, &public_key)?;
    let network = NetworkSettings {
        accelerators_enabled: false,
        ..NetworkSettings::default()
    };
    let http = HttpClient::new(&network)?;
    let provider = ProviderResolver::new(http.clone());
    let mut changed = false;

    for target in TargetId::ALL {
        if target == TargetId::Edge {
            eprintln!("Probe skips Edge because its official URL is transient");
            continue;
        }
        let spec = target.spec();
        for channel in spec.channels {
            for cpu_build in spec.cpu_builds {
                let signed_version = baseline
                    .release(target, *channel, *cpu_build)
                    .map(|release| release.version.as_str());
                match resolve_target(&provider, target, *channel, *cpu_build).await {
                    Ok(descriptor)
                        if version_probe_requires_refresh(&descriptor.version, signed_version) =>
                    {
                        changed = true;
                        eprintln!(
                            "Probe detected {} {} {}: {} -> {}",
                            target.display_name(),
                            channel.display_name(),
                            cpu_build.display_name(),
                            signed_version.unwrap_or("missing"),
                            descriptor.version
                        );
                    }
                    Ok(_) => {}
                    Err(error) => {
                        changed = true;
                        eprintln!(
                            "Probe could not verify {} {} {}: {}; refresh required",
                            target.display_name(),
                            channel.display_name(),
                            cpu_build.display_name(),
                            safe_refresh_error(error.as_ref())
                        );
                    }
                }
            }
        }
    }

    let chrome_plus = runtime::chromeplus::resolve_latest(&http)
        .await
        .map(|release| release.version)
        .map_err(|error| safe_refresh_error(&error));
    let libportable = runtime::libportable::resolve_latest(&http)
        .await
        .map(|release| release.version)
        .map_err(|error| safe_refresh_error(&error));
    changed |= runtime_probe_requires_refresh(&baseline, chrome_plus, libportable);

    println!("changed={changed}");
    Ok(())
}

fn version_probe_requires_refresh(official: &str, signed: Option<&str>) -> bool {
    signed.is_none_or(|signed| {
        !matches!(
            compare_versions(official, signed),
            VersionRelation::Same | VersionRelation::UpstreamNewer
        )
    })
}

fn runtime_probe_requires_refresh(
    baseline: &TrustedMetadata,
    chrome_plus: Result<String, String>,
    libportable: Result<String, String>,
) -> bool {
    let mut changed = false;
    record_runtime_probe(
        "Chrome++",
        baseline
            .chrome_plus
            .as_ref()
            .map(|release| release.version.as_str()),
        chrome_plus,
        &mut changed,
    );
    record_runtime_probe(
        "Libportable",
        baseline
            .libportable
            .as_ref()
            .map(|release| release.version.as_str()),
        libportable,
        &mut changed,
    );
    changed
}

fn record_runtime_probe(
    component: &str,
    signed: Option<&str>,
    official: Result<String, String>,
    changed: &mut bool,
) {
    match official {
        Ok(version) if version_probe_requires_refresh(&version, signed) => {
            *changed = true;
            eprintln!(
                "Probe detected {component}: {} -> {version}",
                signed.unwrap_or("missing")
            );
        }
        Ok(_) => {}
        Err(error) => {
            *changed = true;
            eprintln!("Probe could not verify {component}: {error}; refresh required");
        }
    }
}

async fn refresh_component<T: Clone>(
    name: &str,
    previous: Option<&T>,
    refresh: impl std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>,
) -> Result<T, Box<dyn std::error::Error>> {
    match refresh.await {
        Ok(component) => {
            println!("{name} refresh succeeded");
            Ok(component)
        }
        Err(error) => {
            let reason = safe_refresh_error(error.as_ref());
            match previous {
                Some(previous) => {
                    eprintln!("{name} refresh failed; keeping verified baseline: {reason}");
                    Ok(previous.clone())
                }
                None => Err(
                    format!("{name} refresh failed without a verified baseline: {reason}").into(),
                ),
            }
        }
    }
}

fn safe_refresh_error(error: &dyn std::error::Error) -> String {
    error
        .to_string()
        .split_whitespace()
        .map(|part| {
            if part.contains("://") {
                "[URL redacted]"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

async fn resolve_target(
    provider: &ProviderResolver,
    target: TargetId,
    channel: browser_harbor::domain::release::ReleaseChannel,
    cpu_build: browser_harbor::domain::release::CpuBuild,
) -> Result<ReleaseDescriptor, Box<dyn std::error::Error>> {
    let mut last_error = None;
    for attempt in 1..=2 {
        match provider.resolve(target, channel, cpu_build).await {
            Ok(descriptor) => return Ok(descriptor),
            Err(error) => {
                eprintln!(
                    "{} {} {} attempt {attempt} failed: {}",
                    target.display_name(),
                    channel.display_name(),
                    cpu_build.display_name(),
                    safe_refresh_error(&error)
                );
                last_error = Some(error);
                if attempt < 2 {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    }
    Err(format!(
        "{} {} {} failed: {}",
        target.display_name(),
        channel.display_name(),
        cpu_build.display_name(),
        last_error
            .map(|error| safe_refresh_error(&error))
            .unwrap_or_else(|| "unknown error".to_owned())
    )
    .into())
}

async fn resolve_chrome_plus(
    http: &HttpClient,
) -> Result<ChromePlusRelease, Box<dyn std::error::Error>> {
    let mut last_error = None;
    for attempt in 1..=2 {
        match runtime::chromeplus::resolve_latest(http).await {
            Ok(release) => return Ok(release),
            Err(error) => {
                eprintln!(
                    "Chrome++ attempt {attempt} failed: {}",
                    safe_refresh_error(&error)
                );
                last_error = Some(error);
                if attempt < 2 {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    }
    Err(format!(
        "Chrome++ refresh failed: {}",
        last_error
            .map(|error| safe_refresh_error(&error))
            .unwrap_or_else(|| "unknown error".to_owned())
    )
    .into())
}

async fn resolve_libportable(
    http: &HttpClient,
) -> Result<LibportableRelease, Box<dyn std::error::Error>> {
    let mut last_error = None;
    for attempt in 1..=2 {
        match runtime::libportable::resolve_latest(http).await {
            Ok(release) => return Ok(release),
            Err(error) => {
                eprintln!(
                    "Libportable attempt {attempt} failed: {}",
                    safe_refresh_error(&error)
                );
                last_error = Some(error);
                if attempt < 2 {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    }
    Err(format!(
        "Libportable refresh failed: {}",
        last_error
            .map(|error| safe_refresh_error(&error))
            .unwrap_or_else(|| "unknown error".to_owned())
    )
    .into())
}

async fn ensure_release_integrity(
    mut descriptor: ReleaseDescriptor,
    baseline: &TrustedMetadata,
    client: &reqwest::Client,
    work: &Path,
) -> Result<Option<ReleaseDescriptor>, Box<dyn std::error::Error>> {
    if descriptor
        .artifact
        .urls
        .iter()
        .any(|url| contains_transient_query(url))
    {
        println!(
            "Skipping {} / {} because its live CDP URL is transient",
            descriptor.target.display_name(),
            descriptor.channel.display_name()
        );
        return Ok(None);
    }
    if descriptor.artifact.integrity.is_some() {
        baseline.complete_release_size(&mut descriptor);
        if let Ok(embedded) = TrustedMetadata::load_embedded() {
            embedded.complete_release_size(&mut descriptor);
        }
        return Ok(Some(descriptor));
    }
    if let Some(previous) =
        baseline.release(descriptor.target, descriptor.channel, descriptor.cpu_build)
        && previous.version == descriptor.version
        && previous.artifact.file_name == descriptor.artifact.file_name
        && previous.artifact.integrity.is_some()
    {
        descriptor.artifact.integrity = previous.artifact.integrity.clone();
        baseline.complete_release_size(&mut descriptor);
        if let Ok(embedded) = TrustedMetadata::load_embedded() {
            embedded.complete_release_size(&mut descriptor);
        }
        return Ok(Some(descriptor));
    }
    println!(
        "No trusted digest for {} {} {}; downloading {} to calculate SHA-256",
        descriptor.target.display_name(),
        descriptor.channel.display_name(),
        descriptor.cpu_build.display_name(),
        descriptor.artifact.file_name
    );
    let (_path, file_len, digest) =
        sample_official_artifact(client, &descriptor.artifact, work).await?;
    println!("Sampled {file_len} bytes; calculated SHA-256");
    descriptor.artifact.size = Some(file_len);
    descriptor.artifact.integrity = Some(Integrity::Sha256(digest));
    println!("Calculated SHA-256 for {}", descriptor.artifact.file_name);
    Ok(Some(descriptor))
}

fn contains_transient_query(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("p1=")
        || lower.contains("auth_key=")
        || lower.contains("token=")
        || lower.contains("signature=")
        || lower.contains("sig=")
}

async fn ensure_chrome_plus_integrity(
    mut release: ChromePlusRelease,
    baseline: &TrustedMetadata,
    client: &reqwest::Client,
    work: &Path,
) -> Result<ChromePlusRelease, Box<dyn std::error::Error>> {
    let previous = baseline
        .chrome_plus
        .as_ref()
        .filter(|previous| previous.version == release.version);
    ensure_runtime_artifact(
        &mut release.artifact,
        previous.map(|previous| &previous.artifact),
        client,
        work,
    )
    .await?;
    ensure_runtime_artifact(
        &mut release.setdll_artifact,
        previous.map(|previous| &previous.setdll_artifact),
        client,
        work,
    )
    .await?;
    Ok(release)
}

async fn ensure_libportable_integrity(
    mut release: LibportableRelease,
    baseline: &TrustedMetadata,
    client: &reqwest::Client,
    work: &Path,
) -> Result<LibportableRelease, Box<dyn std::error::Error>> {
    ensure_runtime_artifact(
        &mut release.artifact,
        baseline
            .libportable
            .as_ref()
            .filter(|previous| previous.version == release.version)
            .map(|previous| &previous.artifact),
        client,
        work,
    )
    .await?;
    Ok(release)
}

async fn ensure_runtime_artifact(
    artifact: &mut Artifact,
    previous: Option<&Artifact>,
    client: &reqwest::Client,
    work: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if artifact.integrity.is_some() {
        return Ok(());
    }
    if let Some(previous) = previous
        && previous.file_name == artifact.file_name
        && previous.integrity.is_some()
        && (artifact.size.is_none() || previous.size.is_none() || artifact.size == previous.size)
    {
        artifact.integrity = previous.integrity.clone();
        artifact.size = artifact.size.or(previous.size);
        return Ok(());
    }
    let (_path, size, digest) = sample_official_artifact(client, artifact, work).await?;
    artifact.size = Some(size);
    artifact.integrity = Some(Integrity::Sha256(digest));
    Ok(())
}

async fn sample_official_artifact(
    client: &reqwest::Client,
    artifact: &Artifact,
    work: &Path,
) -> Result<(PathBuf, u64, String), Box<dyn std::error::Error>> {
    let url = artifact
        .urls
        .first()
        .ok_or("official artifact has no source URL")?;
    let response = tokio::time::timeout(
        SAMPLE_TIMEOUT,
        client.get(url).header(ACCEPT_ENCODING, "identity").send(),
    )
    .await??;
    if !response.status().is_success() {
        return Err(format!("official artifact returned HTTP {}", response.status()).into());
    }
    if response
        .headers()
        .get(CONTENT_ENCODING)
        .is_some_and(|value| value != "identity")
    {
        return Err("official artifact used unsupported content encoding".into());
    }
    if let (Some(expected), Some(actual)) = (artifact.size, response.content_length())
        && expected != actual
    {
        return Err(format!(
            "official artifact length mismatch: expected {expected}, received {actual}"
        )
        .into());
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SAMPLE_BYTES)
        || artifact
            .size
            .is_some_and(|length| length > MAX_SAMPLE_BYTES)
    {
        return Err("official artifact exceeds the metadata sampling limit".into());
    }

    fs::create_dir_all(work)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let path = work.join(format!(
        "metadata-sample-{}-{nonce}.part",
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)?;
    let mut hasher = Sha256::new();
    let mut stream = response.bytes_stream();
    let mut received = 0u64;
    let deadline = tokio::time::sleep(SAMPLE_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        let chunk = tokio::select! {
            _ = &mut deadline => return Err("official artifact sampling timed out".into()),
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = chunk else {
            break;
        };
        let chunk = chunk?;
        received = received
            .checked_add(chunk.len() as u64)
            .ok_or("official artifact length overflow")?;
        if received > MAX_SAMPLE_BYTES {
            return Err("official artifact exceeds the metadata sampling limit".into());
        }
        if artifact.size.is_some_and(|expected| received > expected) {
            return Err("official artifact exceeded its trusted length".into());
        }
        file.write_all(&chunk)?;
        hasher.update(&chunk);
    }
    file.flush()?;
    file.sync_all()?;
    if let Some(expected) = artifact.size
        && received != expected
    {
        return Err(format!(
            "official artifact was truncated: expected {expected}, received {received}"
        )
        .into());
    }
    Ok((path, received, hex::encode(hasher.finalize())))
}

#[cfg(test)]
fn sha256_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = std::io::Read::read(&mut file, &mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

struct TempDirectory {
    path: PathBuf,
}

impl TempDirectory {
    fn create() -> Result<Self, Box<dyn std::error::Error>> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let path = env::temp_dir().join(format!(
            "browser-harbor-metadata-refresh-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, atomic::AtomicBool},
        thread,
        time::{Duration, Instant},
    };

    use browser_harbor::{
        domain::{
            release::{
                ArchiveFormat, Artifact, CpuBuild, ProgramPlacement, ProviderKind, ReleaseChannel,
                ReleaseDescriptor,
            },
            settings::NetworkSettings,
            target::TargetId,
        },
        services::{
            ServiceError, download::Downloader, http::HttpClient, trusted_metadata::TrustedMetadata,
        },
    };
    use sha2::{Digest, Sha256};

    fn serve_artifact(bytes: Vec<u8>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            let (mut socket, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            return;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => return,
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 2048];
            while request.len() < 64 * 1024 {
                match socket.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        request.extend_from_slice(&chunk[..read]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(_) => return,
                }
            }
            let mut response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .into_bytes();
            response.extend_from_slice(&bytes);
            let _ = socket.write_all(&response);
        });
        (format!("http://{address}/artifact"), server)
    }

    fn artifact(url: String) -> Artifact {
        Artifact {
            file_name: "new-release.zip".to_owned(),
            format: ArchiveFormat::Zip,
            size: None,
            integrity: None,
            urls: vec![url],
            signature_url: None,
        }
    }

    #[test]
    fn probe_requests_refresh_only_for_missing_newer_or_unparseable_versions() {
        for (official, signed, expected) in [
            ("156.0.8078.81", None, true),
            ("156.0.8078.81", Some("156.0.8078.80"), true),
            ("156.0.8078.81", Some("156.0.8078.81"), false),
            ("156.0.8078.80", Some("156.0.8078.81"), false),
            ("unparseable", Some("156.0.8078.81"), true),
        ] {
            assert_eq!(
                super::version_probe_requires_refresh(official, signed),
                expected,
                "official={official}, signed={signed:?}"
            );
        }
    }

    #[test]
    fn runtime_probe_requests_refresh_for_newer_or_unknown_component_versions() {
        let baseline =
            browser_harbor::services::trusted_metadata::TrustedMetadata::load_embedded().unwrap();
        let chrome_plus = baseline.chrome_plus.as_ref().unwrap().version.clone();
        let libportable = baseline.libportable.as_ref().unwrap().version.clone();
        for (chrome_plus_result, libportable_result, expected) in [
            (Ok(chrome_plus.clone()), Ok(libportable.clone()), false),
            (Ok("999.0.0".to_owned()), Ok(libportable.clone()), true),
            (Ok(chrome_plus.clone()), Ok("999.0.0".to_owned()), true),
            (Err("offline".to_owned()), Ok(libportable.clone()), true),
            (Ok(chrome_plus.clone()), Err("offline".to_owned()), true),
        ] {
            assert_eq!(
                super::runtime_probe_requires_refresh(
                    &baseline,
                    chrome_plus_result,
                    libportable_result,
                ),
                expected
            );
        }
    }

    #[tokio::test]
    async fn refreshing_an_unchanged_release_keeps_its_trusted_size() {
        let mut baseline =
            browser_harbor::services::trusted_metadata::TrustedMetadata::load_embedded().unwrap();
        baseline.releases[0].artifact.size = Some(123);
        let network = browser_harbor::domain::settings::NetworkSettings::default();
        let work = tempfile::tempdir().unwrap();
        let client = HttpClient::download_client(&network).unwrap();
        for without_digest in [false, true] {
            let mut release = baseline.releases[0].clone();
            release.artifact.size = None;
            if without_digest {
                release.artifact.integrity = None;
            }
            let refreshed =
                super::ensure_release_integrity(release, &baseline, &client, work.path())
                    .await
                    .unwrap()
                    .unwrap();
            assert_eq!(refreshed.artifact.size, Some(123));
        }
    }
    use std::fs;

    #[tokio::test]
    async fn metadata_samples_new_release_without_baseline() {
        let bytes = b"new browser release without a signed digest".to_vec();
        let (url, server) = serve_artifact(bytes.clone());
        let network = NetworkSettings {
            accelerators_enabled: false,
            ..NetworkSettings::default()
        };
        let client = HttpClient::download_client(&network).unwrap();
        let work = tempfile::tempdir().unwrap();
        let baseline = TrustedMetadata::load_embedded().unwrap();
        let descriptor = ReleaseDescriptor {
            target: TargetId::Catsxp,
            version: "999.0.0.1".to_owned(),
            channel: ReleaseChannel::Stable,
            cpu_build: CpuBuild::X64,
            provider: ProviderKind::Catsxp,
            artifact: artifact(url),
            placement: ProgramPlacement::App,
            inner_archive: None,
            source: "bounded fixture".to_owned(),
            published_at: None,
        };
        let cache = tempfile::tempdir().unwrap();
        let downloader = Downloader::new(
            client.clone(),
            cache.path().to_path_buf(),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(matches!(
            downloader
                .download(&descriptor.artifact, &NetworkSettings::default(), |_| {})
                .await,
            Err(ServiceError::InvalidIntegrity)
        ));

        let refreshed =
            super::ensure_release_integrity(descriptor, &baseline, &client, work.path())
                .await
                .unwrap()
                .unwrap();
        let expected = hex::encode(Sha256::digest(&bytes));
        assert_eq!(refreshed.artifact.size, Some(bytes.len() as u64));
        assert_eq!(
            refreshed
                .artifact
                .integrity
                .as_ref()
                .map(|value| value.value()),
            Some(expected.as_str())
        );
        server.join().unwrap();
    }

    #[tokio::test]
    async fn metadata_samples_new_runtime_without_baseline() {
        let bytes = b"new runtime without a signed digest".to_vec();
        let (url, server) = serve_artifact(bytes.clone());
        let network = NetworkSettings {
            accelerators_enabled: false,
            ..NetworkSettings::default()
        };
        let client = HttpClient::download_client(&network).unwrap();
        let work = tempfile::tempdir().unwrap();
        let mut runtime = artifact(url);
        runtime.file_name = "new-runtime.7z".to_owned();
        super::ensure_runtime_artifact(&mut runtime, None, &client, work.path())
            .await
            .unwrap();
        let expected = hex::encode(Sha256::digest(&bytes));
        assert_eq!(runtime.size, Some(bytes.len() as u64));
        assert_eq!(
            runtime.integrity.as_ref().map(|value| value.value()),
            Some(expected.as_str())
        );
        server.join().unwrap();
    }

    #[tokio::test]
    async fn reused_runtime_integrity_also_keeps_its_verified_size() {
        let mut runtime = artifact("https://invalid.invalid/not-requested".to_owned());
        let mut previous = runtime.clone();
        previous.size = Some(123);
        previous.integrity = Some(browser_harbor::domain::release::Integrity::Sha256(
            "a".repeat(64),
        ));
        let client = HttpClient::download_client(&NetworkSettings::default()).unwrap();
        super::ensure_runtime_artifact(
            &mut runtime,
            Some(&previous),
            &client,
            tempfile::tempdir().unwrap().path(),
        )
        .await
        .unwrap();
        assert_eq!(runtime.size, Some(123));
        assert_eq!(runtime.integrity, previous.integrity);
    }

    use super::{refresh_component, sha256_file};

    #[tokio::test]
    async fn failed_runtime_refresh_keeps_verified_baseline_and_allows_other_successes() {
        let baseline = super::verify_with_public_key(
            include_bytes!("../../assets/trusted-metadata.json"),
            include_str!("../../assets/trusted-metadata.sig"),
            include_str!("../../assets/metadata-public-key.hex"),
        )
        .unwrap();
        let previous_chrome = baseline.chrome_plus.as_ref().unwrap();
        let previous_libportable = baseline.libportable.as_ref().unwrap();
        let mut new_libportable = previous_libportable.clone();
        new_libportable.version = "new-libportable".to_owned();

        let chrome = refresh_component("Chrome++", Some(previous_chrome), async {
            tokio::task::yield_now().await;
            Err("Chrome++ API unavailable".into())
        })
        .await
        .unwrap();
        let libportable = refresh_component("Libportable", Some(previous_libportable), async {
            Ok(new_libportable)
        })
        .await
        .unwrap();
        assert_eq!(&chrome, previous_chrome);
        assert_eq!(libportable.version, "new-libportable");

        let mut new_chrome = chrome.clone();
        new_chrome.version = "new-chrome-plus".to_owned();
        let chrome = refresh_component("Chrome++", Some(previous_chrome), async { Ok(new_chrome) })
            .await
            .unwrap();
        let libportable = refresh_component("Libportable", Some(previous_libportable), async {
            tokio::task::yield_now().await;
            Err("Libportable artifact unavailable".into())
        })
        .await
        .unwrap();
        assert_eq!(chrome.version, "new-chrome-plus");
        assert_eq!(&libportable, previous_libportable);
    }

    #[tokio::test]
    async fn failed_runtime_without_baseline_reports_the_component() {
        let error = refresh_component::<String>("Chrome++", None, async {
            Err(
                "API unavailable https://user:password@example.com/archive?token=secret#private"
                    .into(),
            )
        })
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("Chrome++"));
        assert!(error.contains("baseline"));
        assert!(error.contains("API unavailable"));
        assert!(!error.contains("password"));
        assert!(!error.contains("secret"));
        assert!(!error.contains("private"));
    }

    #[test]
    fn hashing_large_artifacts_does_not_require_a_large_stack() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.bin");
        let bytes = vec![0x42; 2 * 1024 * 1024];
        fs::write(&path, &bytes).unwrap();
        let expected = hex::encode(Sha256::digest(&bytes));

        let actual = thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(move || sha256_file(&path).map_err(|error| error.to_string()))
            .unwrap()
            .join()
            .unwrap()
            .unwrap();

        assert_eq!(actual, expected);
    }
}
