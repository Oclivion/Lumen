//! Key generation tool for Lumen release signing

use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;

fn main() {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();

    let private_hex = hex::encode(signing_key.to_bytes());
    let public_hex = hex::encode(verifying_key.to_bytes());

    eprintln!("=== Lumen Release Signing Keypair ===\n");
    eprintln!("PRIVATE KEY (keep secret!):");
    eprintln!("{}\n", private_hex);
    eprintln!("PUBLIC KEY (embed in code):");
    eprintln!("{}\n", public_hex);
    eprintln!("IMPORTANT:");
    eprintln!("1. Store PRIVATE_KEY in GitHub Secrets as LUMEN_SIGNING_KEY");
    eprintln!("2. Update orchestrator/src/config.rs UpdateConfig::public_key");
    eprintln!("3. NEVER commit the private key to version control!");

    // Also output in machine-readable format
    println!("LUMEN_PRIVATE_KEY={}", private_hex);
    println!("LUMEN_PUBLIC_KEY={}", public_hex);
}
