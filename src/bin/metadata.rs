use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, atomic::AtomicBool},
    time::{SystemTime, UNIX_EPOCH},
};

use browser_harbor::{
    domain::{
        release::{Artifact, Integrity, ReleaseDescriptor},
        runtime::{ChromePlusRelease, LibportableRelease},
        settings::NetworkSettings,
        target::TargetId,
    },
    providers::ProviderResolver,
    runtime,
    services::{
        download::Downloader,
        http::HttpClient,
        trusted_metadata::{NO_EXPIRY, TrustedMetadata, verify_with_public_key},
    },
};
use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;
use sha2::{Digest, Sha256};

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
        _ => Err("usage: metadata keygen|sign|sign-env|verify|refresh [--path VALUE]".into()),
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
    let (direct, fallback) = HttpClient::download_clients(&network)?;
    let downloader = Downloader::with_fallback_clients(
        direct,
        fallback,
        work.path.clone(),
        Arc::new(AtomicBool::new(false)),
    );

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
                    ensure_release_integrity(descriptor, &baseline, &downloader, &network).await
                }
                .await;
                match refreshed {
                    Ok(Some(descriptor)) => releases.push(descriptor),
                    Ok(None) => {}
                    Err(error) => {
                        if let Some(previous) = baseline.release(target, *channel, *cpu_build) {
                            eprintln!(
                                "{} / {} / {} refresh failed; keeping baseline: {error}",
                                target.display_name(),
                                channel.display_name(),
                                cpu_build.display_name()
                            );
                            releases.push(previous.clone());
                        } else {
                            return Err(error);
                        }
                    }
                }
            }
        }
    }

    println!("Refreshing Chrome++");
    let chrome_plus = ensure_chrome_plus_integrity(
        resolve_chrome_plus(&http).await?,
        &baseline,
        &downloader,
        &network,
    )
    .await?;
    println!("Refreshing Libportable");
    let libportable = ensure_libportable_integrity(
        resolve_libportable(&http).await?,
        &baseline,
        &downloader,
        &network,
    )
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
                    "{} {} {} attempt {attempt} failed: {error}",
                    target.display_name(),
                    channel.display_name(),
                    cpu_build.display_name()
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
            .map(|error| error.to_string())
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
                eprintln!("Chrome++ attempt {attempt} failed: {error}");
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
            .map(|error| error.to_string())
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
                eprintln!("Libportable attempt {attempt} failed: {error}");
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
            .map(|error| error.to_string())
            .unwrap_or_else(|| "unknown error".to_owned())
    )
    .into())
}

async fn ensure_release_integrity(
    mut descriptor: ReleaseDescriptor,
    baseline: &TrustedMetadata,
    downloader: &Downloader,
    network: &NetworkSettings,
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
        return Ok(Some(descriptor));
    }
    if let Some(previous) =
        baseline.release(descriptor.target, descriptor.channel, descriptor.cpu_build)
        && previous.version == descriptor.version
        && previous.artifact.file_name == descriptor.artifact.file_name
        && previous.artifact.integrity.is_some()
    {
        descriptor.artifact.integrity = previous.artifact.integrity.clone();
        return Ok(Some(descriptor));
    }
    println!(
        "No trusted digest for {} {} {}; downloading {} to calculate SHA-256",
        descriptor.target.display_name(),
        descriptor.channel.display_name(),
        descriptor.cpu_build.display_name(),
        descriptor.artifact.file_name
    );
    let path = downloader
        .download(&descriptor.artifact, network, |_| {})
        .await?;
    let file_len = fs::metadata(&path)?.len();
    println!("Downloaded {file_len} bytes; calculating SHA-256");
    if descriptor.artifact.size.is_none() {
        descriptor.artifact.size = Some(file_len);
    }
    descriptor.artifact.integrity = Some(Integrity::Sha256(sha256_file(&path)?));
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
    downloader: &Downloader,
    network: &NetworkSettings,
) -> Result<ChromePlusRelease, Box<dyn std::error::Error>> {
    let previous = baseline
        .chrome_plus
        .as_ref()
        .filter(|previous| previous.version == release.version);
    ensure_runtime_artifact(
        &mut release.artifact,
        previous.map(|previous| &previous.artifact),
        downloader,
        network,
    )
    .await?;
    ensure_runtime_artifact(
        &mut release.setdll_artifact,
        previous.map(|previous| &previous.setdll_artifact),
        downloader,
        network,
    )
    .await?;
    Ok(release)
}

async fn ensure_libportable_integrity(
    mut release: LibportableRelease,
    baseline: &TrustedMetadata,
    downloader: &Downloader,
    network: &NetworkSettings,
) -> Result<LibportableRelease, Box<dyn std::error::Error>> {
    ensure_runtime_artifact(
        &mut release.artifact,
        baseline
            .libportable
            .as_ref()
            .filter(|previous| previous.version == release.version)
            .map(|previous| &previous.artifact),
        downloader,
        network,
    )
    .await?;
    Ok(release)
}

async fn ensure_runtime_artifact(
    artifact: &mut Artifact,
    previous: Option<&Artifact>,
    downloader: &Downloader,
    network: &NetworkSettings,
) -> Result<(), Box<dyn std::error::Error>> {
    if artifact.integrity.is_some() {
        return Ok(());
    }
    if let Some(previous) = previous
        && previous.file_name == artifact.file_name
        && previous.integrity.is_some()
    {
        artifact.integrity = previous.integrity.clone();
        return Ok(());
    }
    let path = downloader.download(artifact, network, |_| {}).await?;
    artifact.integrity = Some(Integrity::Sha256(sha256_file(&path)?));
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
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
    use std::{fs, thread};

    use sha2::{Digest, Sha256};

    use super::sha256_file;

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
