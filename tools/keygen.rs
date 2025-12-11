//! Key generation tool for Lumen release signing
//!
//! Run with: cargo run --bin keygen

use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;

fn main() {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();

    let private_hex = hex::encode(signing_key.to_bytes());
    let public_hex = hex::encode(verifying_key.to_bytes());

    println!("=== Lumen Release Signing Keypair ===\n");
    println!("PRIVATE KEY (keep secret!):");
    println!("{}\n", private_hex);
    println!("PUBLIC KEY (embed in code):");
    println!("{}\n", public_hex);
    println!("IMPORTANT:");
    println!("1. Store PRIVATE_KEY in GitHub Secrets as LUMEN_SIGNING_KEY");
    println!("2. Update orchestrator/src/config.rs UpdateConfig::public_key");
    println!("3. NEVER commit the private key to version control!");
}
