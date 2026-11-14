//! SHA-256 and ECDSA P-256 signature checks through Windows CNG (bcrypt.dll), so updates need no extra crates.
//! Also compiled into the `kestrel-sign` release tool, which is why nothing here depends on the rest of Kestrel.

use windows::Win32::Foundation::NTSTATUS;
use windows::Win32::Security::Cryptography::{
    BCryptDestroyKey, BCryptHash, BCryptImportKeyPair, BCryptVerifySignature, BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALG_HANDLE,
    BCRYPT_FLAGS, BCRYPT_KEY_HANDLE, BCRYPT_SHA256_ALG_HANDLE,
};

pub fn check(status: NTSTATUS, what: &str) -> Result<(), String> {
    status.ok().map_err(|e| format!("{what} failed: {}", e.message()))
}

pub fn sha256(data: &[u8]) -> Result<[u8; 32], String> {
    let mut out = [0u8; 32];
    check(unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) }, "SHA-256")?;
    Ok(out)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// The exact text a release signature covers. Binding the version stops an old, validly signed build from being
/// offered as a newer one.
pub fn message(version: &str, sha256_hex: &str) -> String {
    format!("kestrel-update\nversion={version}\nsha256={sha256_hex}\n")
}

/// True only if `signature` (64 bytes, r||s) is a valid P-256 signature of SHA-256(`message`) by `public_blob`
/// (a BCRYPT_ECCPUBLIC_BLOB).
pub fn verify(public_blob: &[u8], message: &[u8], signature: &[u8]) -> bool {
    let Ok(hash) = sha256(message) else { return false };
    let mut key = BCRYPT_KEY_HANDLE::default();
    unsafe {
        let imported = BCryptImportKeyPair(
            BCRYPT_ECDSA_P256_ALG_HANDLE,
            BCRYPT_KEY_HANDLE::default(),
            BCRYPT_ECCPUBLIC_BLOB,
            &mut key,
            public_blob,
            0,
        );
        if imported.is_err() {
            return false;
        }
        let ok = BCryptVerifySignature(key, None, &hash, signature, BCRYPT_FLAGS(0)).is_ok();
        let _ = BCryptDestroyKey(key);
        ok
    }
}
