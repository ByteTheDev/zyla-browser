//! Locally stored passwords and payment cards, protected with Windows DPAPI.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use zeroize::Zeroize;

const MAGIC: &[u8] = b"ZYLA-VAULT-1\0";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Vault {
    #[serde(default)]
    pub passwords: Vec<SavedPassword>,
    #[serde(default)]
    pub cards: Vec<SavedCard>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPassword {
    pub id: u64,
    pub host: String,
    pub username: String,
    pub password: String,
}

impl Drop for SavedPassword {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedCard {
    pub id: u64,
    pub name: String,
    pub number: String,
    pub exp_month: u8,
    pub exp_year: u16,
}

impl Drop for SavedCard {
    fn drop(&mut self) {
        self.number.zeroize();
    }
}

pub fn load(path: &Path) -> Result<Vault> {
    let protected = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vault::default()),
        Err(error) => return Err(error).context("Could not read the password vault"),
    };
    let Some(ciphertext) = protected.strip_prefix(MAGIC) else {
        bail!("The password vault has an unsupported format")
    };

    let mut plaintext = unprotect(ciphertext).context("Could not unlock the password vault for this Windows account")?;
    let result = serde_json::from_slice(&plaintext).context("The password vault data is damaged");
    plaintext.fill(0);
    result
}

pub fn save(path: &Path, vault: &Vault) -> Result<()> {
    let mut plaintext = serde_json::to_vec(vault).context("Could not encode the password vault")?;
    let encrypted = protect(&plaintext).context("Could not encrypt the password vault")?;
    plaintext.fill(0);

    let mut output = Vec::with_capacity(MAGIC.len() + encrypted.len());
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&encrypted);
    let parent = path.parent().context("The vault path has no parent directory")?;
    fs::create_dir_all(parent).context("Could not create the browser data directory")?;
    let temp = path.with_extension("bin.tmp");
    fs::write(&temp, output).context("Could not write the encrypted password vault")?;
    fs::rename(&temp, path).context("Could not replace the encrypted password vault")
}

#[cfg(windows)]
fn protect(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::{ffi::c_void, io};

    #[repr(C)]
    struct DataBlob {
        size: u32,
        data: *mut u8,
    }

    #[link(name = "Crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            input: *const DataBlob,
            description: *const u16,
            entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    let size = u32::try_from(bytes.len()).context("Vault is too large to encrypt")?;
    let input = DataBlob { size, data: bytes.as_ptr() as *mut u8 };
    let mut output = DataBlob { size: 0, data: std::ptr::null_mut() };
    // CRYPTPROTECT_UI_FORBIDDEN keeps encryption non-interactive; omitting LOCAL_MACHINE
    // binds the result to the current Windows user.
    let ok = unsafe {
        CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), 1, &mut output)
    };
    if ok == 0 {
        return Err(io::Error::last_os_error()).context("Windows DPAPI encryption failed");
    }
    let encrypted = unsafe { std::slice::from_raw_parts(output.data, output.size as usize).to_vec() };
    unsafe { LocalFree(output.data.cast()); }
    Ok(encrypted)
}

#[cfg(windows)]
fn unprotect(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::{ffi::c_void, io};

    #[repr(C)]
    struct DataBlob {
        size: u32,
        data: *mut u8,
    }

    #[link(name = "Crypt32")]
    unsafe extern "system" {
        fn CryptUnprotectData(
            input: *const DataBlob,
            description: *mut *mut u16,
            entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    let size = u32::try_from(bytes.len()).context("Vault is too large to decrypt")?;
    let input = DataBlob { size, data: bytes.as_ptr() as *mut u8 };
    let mut output = DataBlob { size: 0, data: std::ptr::null_mut() };
    let ok = unsafe {
        CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), 1, &mut output)
    };
    if ok == 0 {
        return Err(io::Error::last_os_error()).context("Windows DPAPI decryption failed");
    }
    let plaintext = unsafe { std::slice::from_raw_parts(output.data, output.size as usize).to_vec() };
    unsafe { LocalFree(output.data.cast()); }
    Ok(plaintext)
}

#[cfg(not(windows))]
fn protect(_bytes: &[u8]) -> Result<Vec<u8>> {
    bail!("The password vault is supported on Windows only")
}

#[cfg(not(windows))]
fn unprotect(_bytes: &[u8]) -> Result<Vec<u8>> {
    bail!("The password vault is supported on Windows only")
}
