//! Encryption for environment secrets (spec §4.9, §9.1).
//!
//! XChaCha20-Poly1305 with a random 24-byte nonce per value. The
//! environment ID and secret name are authenticated as associated data, so
//! ciphertext cannot be moved to another environment or name.

use std::path::Path;

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use workbench_core::EnvironmentId;
use workbench_storage::EncryptedSecret;

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;

/// The key that encrypts secrets at rest. Keep it out of the database.
#[derive(Clone)]
pub struct SecretKey([u8; KEY_LEN]);

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey(..)")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecretKeyError {
    #[error("secret key must be {KEY_LEN} bytes ({} hex characters)", KEY_LEN * 2)]
    InvalidLength,
    #[error("secret key is not valid hex")]
    InvalidHex,
    #[error("cannot read or create key file {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("system random number generator failed")]
    Random,
}

fn random<const N: usize>() -> Result<[u8; N], SecretKeyError> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|_| SecretKeyError::Random)?;
    Ok(bytes)
}

fn aad(environment: EnvironmentId, name: &str) -> Vec<u8> {
    let mut aad = environment.as_uuid().as_bytes().to_vec();
    aad.extend_from_slice(name.as_bytes());
    aad
}

impl SecretKey {
    pub fn generate() -> Result<Self, SecretKeyError> {
        random().map(Self)
    }

    /// Parses 64 hex characters.
    pub fn from_hex(hex: &str) -> Result<Self, SecretKeyError> {
        let hex = hex.trim();
        if hex.len() != KEY_LEN * 2 {
            return Err(SecretKeyError::InvalidLength);
        }
        let mut key = [0u8; KEY_LEN];
        for (i, byte) in key.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .map_err(|_| SecretKeyError::InvalidHex)?;
        }
        Ok(Self(key))
    }

    /// Reads a raw 32-byte key file, creating it with a fresh key if missing.
    pub fn load_or_create(path: &Path) -> Result<Self, SecretKeyError> {
        let io = |source| SecretKeyError::Io {
            path: path.display().to_string(),
            source,
        };
        match std::fs::read(path) {
            Ok(bytes) => <[u8; KEY_LEN]>::try_from(bytes.as_slice())
                .map(Self)
                .map_err(|_| SecretKeyError::InvalidLength),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let key = Self::generate()?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(io)?;
                }
                write_private(path, &key.0).map_err(io)?;
                Ok(key)
            }
            Err(e) => Err(io(e)),
        }
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(&self.0.into())
    }

    pub fn encrypt(
        &self,
        environment: EnvironmentId,
        name: &str,
        plaintext: &str,
    ) -> Result<EncryptedSecret, SecretKeyError> {
        let nonce: [u8; NONCE_LEN] = random()?;
        let aad = aad(environment, name);
        let ciphertext = self
            .cipher()
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: &aad,
                },
            )
            .map_err(|_| SecretKeyError::Random)?;
        Ok(EncryptedSecret {
            nonce: nonce.to_vec(),
            ciphertext,
        })
    }

    /// `None` if the value was not encrypted with this key for this location.
    #[must_use]
    pub fn decrypt(
        &self,
        environment: EnvironmentId,
        name: &str,
        secret: &EncryptedSecret,
    ) -> Option<String> {
        let nonce = <[u8; NONCE_LEN]>::try_from(secret.nonce.as_slice()).ok()?;
        let aad = aad(environment, name);
        let plain = self
            .cipher()
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &secret.ciphertext,
                    aad: &aad,
                },
            )
            .ok()?;
        String::from_utf8(plain).ok()
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_binding() {
        let key = SecretKey::generate().unwrap();
        let env = EnvironmentId::new();
        let sealed = key.encrypt(env, "API_TOKEN", "s3cr3t").unwrap();
        assert!(
            !sealed.ciphertext.windows(6).any(|w| w == b"s3cr3t"),
            "no plaintext in ciphertext"
        );
        assert_eq!(
            key.decrypt(env, "API_TOKEN", &sealed).as_deref(),
            Some("s3cr3t")
        );
        assert_eq!(
            key.decrypt(env, "OTHER", &sealed),
            None,
            "bound to the name"
        );
        assert_eq!(
            key.decrypt(EnvironmentId::new(), "API_TOKEN", &sealed),
            None,
            "bound to the environment"
        );
        assert_eq!(
            SecretKey::generate()
                .unwrap()
                .decrypt(env, "API_TOKEN", &sealed),
            None,
            "wrong key"
        );
        let again = key.encrypt(env, "API_TOKEN", "s3cr3t").unwrap();
        assert_ne!(again.nonce, sealed.nonce, "fresh nonce every time");
    }

    #[test]
    fn hex_and_key_files() {
        let hex = "00".repeat(31) + "ff";
        assert!(SecretKey::from_hex(&hex).is_ok());
        assert!(matches!(
            SecretKey::from_hex("abc"),
            Err(SecretKeyError::InvalidLength)
        ));
        assert!(matches!(
            SecretKey::from_hex(&"zz".repeat(32)),
            Err(SecretKeyError::InvalidHex)
        ));

        let path = std::env::temp_dir().join(format!("wb-key-{}/secret.key", uuid::Uuid::new_v4()));
        let first = SecretKey::load_or_create(&path).unwrap();
        let second = SecretKey::load_or_create(&path).unwrap();
        assert_eq!(first.0, second.0, "the file is reused");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
