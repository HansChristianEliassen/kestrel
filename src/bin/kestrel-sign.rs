//! Release signing tool. Not part of kestrel.exe and never shipped.
//!
//!   kestrel-sign keygen <key file>                         new P-256 key pair; prints the public key to embed
//!   kestrel-sign sign <key file> <exe> <version> <out.sig>  writes the signature file to publish with the exe
//!
//! The key file holds the private key: keep it out of the repository and back it up. Without it no further
//! update can be signed, and anyone who has it can sign updates.

#[allow(dead_code)]
#[path = "../update/crypto.rs"]
mod crypto;

use std::fs;

use windows::Win32::Security::Cryptography::{
    BCryptDestroyKey, BCryptExportKey, BCryptFinalizeKeyPair, BCryptGenerateKeyPair, BCryptImportKeyPair, BCryptSignHash,
    BCRYPT_ECCPRIVATE_BLOB, BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALG_HANDLE, BCRYPT_FLAGS, BCRYPT_KEY_HANDLE,
};

fn export(key: BCRYPT_KEY_HANDLE, kind: windows::core::PCWSTR) -> Result<Vec<u8>, String> {
    let mut len = 0u32;
    unsafe {
        crypto::check(BCryptExportKey(key, BCRYPT_KEY_HANDLE::default(), kind, None, &mut len, 0), "Export size")?;
        let mut out = vec![0u8; len as usize];
        crypto::check(BCryptExportKey(key, BCRYPT_KEY_HANDLE::default(), kind, Some(&mut out), &mut len, 0), "Export")?;
        out.truncate(len as usize);
        Ok(out)
    }
}

fn keygen(path: &str) -> Result<(), String> {
    if fs::metadata(path).is_ok() {
        return Err(format!("{path} already exists; refusing to overwrite a signing key."));
    }
    let mut key = BCRYPT_KEY_HANDLE::default();
    unsafe {
        crypto::check(BCryptGenerateKeyPair(BCRYPT_ECDSA_P256_ALG_HANDLE, &mut key, 256, 0), "Key generation")?;
        crypto::check(BCryptFinalizeKeyPair(key, 0), "Key generation")?;
    }
    let (private, public) = (export(key, BCRYPT_ECCPRIVATE_BLOB), export(key, BCRYPT_ECCPUBLIC_BLOB));
    unsafe {
        let _ = BCryptDestroyKey(key);
    }
    if let Some(dir) = std::path::Path::new(path).parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(path, private?).map_err(|e| e.to_string())?;
    println!("Private key written to {path}. Back it up somewhere safe and never commit it.");
    println!("Public key (put this in src/update/mod.rs, PUBLIC_KEY):\n{}", crypto::hex(&public?));
    Ok(())
}

fn sign(key_path: &str, exe: &str, version: &str, out: &str) -> Result<(), String> {
    let private = fs::read(key_path).map_err(|e| format!("Couldn't read the key: {e}"))?;
    let data = fs::read(exe).map_err(|e| format!("Couldn't read {exe}: {e}"))?;
    let message = crypto::message(version, &crypto::hex(&crypto::sha256(&data)?));
    let hash = crypto::sha256(message.as_bytes())?;
    let mut key = BCRYPT_KEY_HANDLE::default();
    let mut signature = [0u8; 64];
    let mut len = 0u32;
    unsafe {
        crypto::check(
            BCryptImportKeyPair(BCRYPT_ECDSA_P256_ALG_HANDLE, BCRYPT_KEY_HANDLE::default(), BCRYPT_ECCPRIVATE_BLOB, &mut key, &private, 0),
            "Key import",
        )?;
        let signed = BCryptSignHash(key, None, &hash, Some(&mut signature), &mut len, BCRYPT_FLAGS(0));
        let public = export(key, BCRYPT_ECCPUBLIC_BLOB);
        let _ = BCryptDestroyKey(key);
        crypto::check(signed, "Signing")?;
        // Check the result the same way Kestrel will before publishing anything.
        if !crypto::verify(&public?, message.as_bytes(), &signature[..len as usize]) {
            return Err("The new signature didn't verify; nothing was written.".into());
        }
    }
    fs::write(out, format!("{message}signature={}\n", crypto::hex(&signature[..len as usize]))).map_err(|e| e.to_string())?;
    println!("Signed {exe} as version {version} -> {out}");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["keygen", path] => keygen(path),
        ["sign", key, exe, version, out] => sign(key, exe, version, out),
        _ => Err("Usage: kestrel-sign keygen <key file> | kestrel-sign sign <key file> <exe> <version> <out.sig>".into()),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
