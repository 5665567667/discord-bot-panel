use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use anyhow::{Result, bail};
use rand::{RngCore, rngs::OsRng};

pub fn seal(key: &[u8; 32], plaintext: &str) -> Result<String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| anyhow::anyhow!("invalid encryption key"))?;
    let mut nonce_bytes = [0_u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes()).map_err(|_| anyhow::anyhow!("token encryption failed"))?;
    let mut value = nonce_bytes.to_vec();
    value.extend(ciphertext);
    Ok(hex::encode(value))
}

pub fn open(key: &[u8; 32], value: &str) -> Result<String> {
    let bytes = hex::decode(value)?;
    if bytes.len() < 13 { bail!("encrypted value is too short"); }
    let (nonce, ciphertext) = bytes.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| anyhow::anyhow!("invalid encryption key"))?;
    let plaintext = cipher.decrypt(Nonce::from_slice(nonce), ciphertext).map_err(|_| anyhow::anyhow!("token decryption failed"))?;
    Ok(String::from_utf8(plaintext)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encryption_round_trip_and_authentication() {
        let key = [7_u8; 32];
        let sealed = seal(&key, "private token").unwrap();
        assert_eq!(open(&key, &sealed).unwrap(), "private token");
        assert!(open(&[8_u8; 32], &sealed).is_err());
    }
}
