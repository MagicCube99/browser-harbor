use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use browser_harbor::services::trusted_metadata::verify_with_public_key;
use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
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
        Some("verify") => {
            let manifest = required_path(&mut args, "--manifest")?;
            let signature = required_path(&mut args, "--signature")?;
            let public_key = required_path(&mut args, "--public-key")?;
            verify(&manifest, &signature, &public_key)
        }
        _ => Err("usage: metadata keygen|sign|verify [--path VALUE]".into()),
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
    let private_key = hex::decode(private_key.trim())?;
    let private_key: [u8; 32] = private_key
        .try_into()
        .map_err(|_| "private key must contain 32 bytes")?;
    let key = SigningKey::from_bytes(&private_key);
    let signature = hex::encode(key.sign(&bytes).to_bytes());
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
    let actual = args.next().ok_or_else(|| format!("missing flag {flag}"))?;
    if actual != flag {
        return Err(format!("expected flag {flag}, found {actual}").into());
    }
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing value after {flag}").into())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}
