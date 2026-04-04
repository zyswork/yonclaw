//! 字段级混淆 — 防止数据库明文暴露 API Key
//!
//! 使用 XOR + base64 简单混淆。不是强加密，但足以：
//! - 防止 `strings xianzhu.db` 直接看到 API Key
//! - 防止导出 zip 时意外泄露（导出已有脱敏，这是额外防线）
//! - 桌面单用户场景下安全够用

use sha2::{Sha256, Digest};

/// 加密前缀标记
const PREFIX: &str = "XZ1:";

/// 从设备相关信息派生混淆密钥
fn derive_key() -> Vec<u8> {
    let mut hasher = Sha256::new();
    // 用 home 目录 + 固定盐作为密钥（每台机器不同）
    let home = dirs::home_dir().unwrap_or_default();
    hasher.update(home.to_string_lossy().as_bytes());
    hasher.update(b"xianzhu-field-obfuscation-v1");
    hasher.finalize().to_vec()
}

/// 混淆字段（XOR + base64）
pub fn encrypt_field(plaintext: &str) -> String {
    if plaintext.is_empty() || plaintext.starts_with(PREFIX) {
        return plaintext.to_string();
    }
    let key = derive_key();
    let xored: Vec<u8> = plaintext.bytes()
        .enumerate()
        .map(|(i, b)| b ^ key[i % key.len()])
        .collect();
    format!("{}{}", PREFIX, base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &xored))
}

/// 解混淆字段
pub fn decrypt_field(ciphertext: &str) -> String {
    if !ciphertext.starts_with(PREFIX) {
        return ciphertext.to_string(); // 未混淆，兼容旧数据
    }
    let encoded = &ciphertext[PREFIX.len()..];
    let xored = match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded) {
        Ok(bytes) => bytes,
        Err(_) => return ciphertext.to_string(),
    };
    let key = derive_key();
    let plain: Vec<u8> = xored.iter()
        .enumerate()
        .map(|(i, b)| b ^ key[i % key.len()])
        .collect();
    String::from_utf8(plain).unwrap_or_else(|_| ciphertext.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let original = "sk-test-key-12345-abcdef";
        let encrypted = encrypt_field(original);
        assert!(encrypted.starts_with("XZ1:"));
        assert_ne!(encrypted, original);
        assert_eq!(decrypt_field(&encrypted), original);
    }

    #[test]
    fn test_empty_passthrough() {
        assert_eq!(encrypt_field(""), "");
        assert_eq!(encrypt_field("XZ1:abc"), "XZ1:abc");
    }

    #[test]
    fn test_plaintext_passthrough() {
        assert_eq!(decrypt_field("sk-plain"), "sk-plain");
    }
}
