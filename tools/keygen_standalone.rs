use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
fn main() {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();
    println!("PRIVATE_KEY={}", hex::encode(signing_key.to_bytes()));
    println!("PUBLIC_KEY={}", hex::encode(verifying_key.to_bytes()));
}
