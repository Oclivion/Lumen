use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: verify_release <public_key_file> <file_to_verify> [version.json]");
        std::process::exit(1);
    }

    let key_file = &args[1];
    let file_to_verify = &args[2];
    let manifest_file = args.get(3);

    // Read public key (hex encoded)
    let public_key_hex = fs::read_to_string(key_file)?
        .trim()
        .to_string();

    let public_bytes = hex::decode(&public_key_hex)?;
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&public_bytes);
    let verifying_key = VerifyingKey::from_bytes(&key_bytes)?;

    // Read file and compute SHA256
    let file_data = fs::read(file_to_verify)?;
    let mut hasher = Sha256::new();
    hasher.update(&file_data);
    let hash = hasher.finalize();
    let computed_sha256 = hex::encode(&hash);

    // Get signature from manifest or command line
    let (expected_sha256, signature_hex) = if let Some(mf) = manifest_file {
        let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(mf)?)?;
        (
            manifest["sha256"].as_str().unwrap().to_string(),
            manifest["signature"].as_str().unwrap().to_string(),
        )
    } else {
        eprintln!("ERROR: version.json manifest file required");
        std::process::exit(1);
    };

    // Verify SHA256 matches
    if computed_sha256 != expected_sha256 {
        eprintln!("❌ SHA256 MISMATCH!");
        eprintln!("   Expected: {}", expected_sha256);
        eprintln!("   Computed: {}", computed_sha256);
        std::process::exit(1);
    }

    println!("✓ SHA256 verified: {}", computed_sha256);

    // Verify signature
    let signature_bytes = hex::decode(&signature_hex)?;
    let signature = Signature::from_slice(&signature_bytes)?;

    match verifying_key.verify(&hash, &signature) {
        Ok(()) => {
            println!("✓ Signature verified!");
            println!("");
            println!("Release is authentic and unmodified.");
        }
        Err(e) => {
            eprintln!("❌ SIGNATURE VERIFICATION FAILED: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}
