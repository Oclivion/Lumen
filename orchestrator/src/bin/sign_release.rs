use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: sign_release <private_key_file> <file_to_sign>");
        eprintln!("");
        eprintln!("Signs a file with Ed25519 and outputs JSON manifest");
        std::process::exit(1);
    }

    let key_file = &args[1];
    let file_to_sign = &args[2];
    let version = args.get(3).map(|s| s.as_str()).unwrap_or("0.1.0");

    // Read private key (hex encoded)
    let private_key_hex = fs::read_to_string(key_file)?
        .trim()
        .to_string();

    let private_bytes = hex::decode(&private_key_hex)?;
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&private_bytes);
    let signing_key = SigningKey::from_bytes(&key_bytes);

    // Read file and compute SHA256
    let file_data = fs::read(file_to_sign)?;
    let mut hasher = Sha256::new();
    hasher.update(&file_data);
    let hash = hasher.finalize();
    let sha256_hex = hex::encode(&hash);

    // Sign the hash
    let signature = signing_key.sign(&hash);
    let signature_hex = hex::encode(signature.to_bytes());

    // Get file size
    let size = file_data.len();

    // Get filename
    let filename = Path::new(file_to_sign)
        .file_name()
        .unwrap()
        .to_string_lossy();

    // Output version.json
    let manifest = format!(r#"{{
  "version": "{}",
  "sha256": "{}",
  "signature": "{}",
  "min_version": null,
  "release_notes": "Lumen v{}",
  "released_at": "{}",
  "downloads": {{
    "linux_x86_64": "https://github.com/Oclivion/lumen/releases/download/v{}/{}",
    "linux_aarch64": null,
    "darwin_x86_64": null,
    "darwin_aarch64": null,
    "windows_x86_64": null
  }},
  "size": {}
}}"#,
        version,
        sha256_hex,
        signature_hex,
        version,
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        version,
        filename,
        size
    );

    println!("{}", manifest);

    eprintln!("");
    eprintln!("SHA256:    {}", sha256_hex);
    eprintln!("Signature: {}...", &signature_hex[..64]);
    eprintln!("Size:      {} bytes", size);

    Ok(())
}
