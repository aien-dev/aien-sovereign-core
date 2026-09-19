//! Hardware TPM 2.0 secret provider implementation for AIEN.

use crate::secret::{
    SealedSecret, SecretCapabilities, SecretError, SecretProtectionLevel, SecretProvider,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use zeroize::Zeroizing;

/// Hardware TPM 2.0 provider backed by platform sealed storage.
pub struct Tpm2SecretProvider {
    vault_binary: PathBuf,
    tpm_device: PathBuf,
}

impl Tpm2SecretProvider {
    /// Probe system for hardware TPM availability.
    pub fn probe() -> Self {
        let default_device = PathBuf::from("/dev/tpmrm0");
        let default_vault = if let Ok(home) = std::env::var("HOME") {
            let candidate = Path::new(&home).join(".local/bin/atlas-vault");
            if candidate.exists() {
                candidate
            } else {
                PathBuf::from("atlas-vault")
            }
        } else {
            PathBuf::from("atlas-vault")
        };

        Self {
            vault_binary: default_vault,
            tpm_device: default_device,
        }
    }

    /// Check if the hardware TPM device node exists on this system.
    pub fn is_hardware_present(&self) -> bool {
        self.tpm_device.exists()
    }
}

impl Default for Tpm2SecretProvider {
    fn default() -> Self {
        Self::probe()
    }
}

impl SecretProvider for Tpm2SecretProvider {
    fn capabilities(&self) -> SecretCapabilities {
        SecretCapabilities {
            protection_level: SecretProtectionLevel::HardwareBackedSealed,
            can_seal: true,
            can_unseal: true,
            hardware_bound: true,
            persistent_at_rest: true,
        }
    }

    fn protection_level(&self) -> SecretProtectionLevel {
        SecretProtectionLevel::HardwareBackedSealed
    }

    fn seal(&self, name: &str, secret: &[u8]) -> Result<SealedSecret, SecretError> {
        let secret_str = std::str::from_utf8(secret).map_err(|_| {
            SecretError::CryptoError("TPM vault requires UTF-8 compatible secret input".to_string())
        })?;

        let output = Command::new(&self.vault_binary)
            .args(["add", name, secret_str])
            .output()
            .map_err(|e| SecretError::IoError(format!("Failed to execute vault command: {}", e)))?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            return Err(SecretError::CryptoError(format!(
                "TPM vault add failed: {}",
                err_msg.trim()
            )));
        }

        Ok(SealedSecret {
            name: name.to_string(),
            ciphertext: Vec::new(),
            protection_level: SecretProtectionLevel::HardwareBackedSealed,
            metadata: HashMap::new(),
        })
    }

    fn unseal(&self, secret: &SealedSecret) -> Result<Zeroizing<Vec<u8>>, SecretError> {
        self.resolve(&secret.name)
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        let output = Command::new(&self.vault_binary)
            .args(["delete", name])
            .output()
            .map_err(|e| SecretError::IoError(format!("Failed to execute vault command: {}", e)))?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            return Err(SecretError::CryptoError(format!(
                "TPM vault delete failed: {}",
                err_msg.trim()
            )));
        }

        Ok(())
    }

    fn resolve(&self, name: &str) -> Result<Zeroizing<Vec<u8>>, SecretError> {
        let output = Command::new(&self.vault_binary)
            .args(["get", name])
            .output()
            .map_err(|e| SecretError::IoError(format!("Failed to execute vault command: {}", e)))?;

        if !output.status.success() {
            return Err(SecretError::NotFound(name.to_string()));
        }

        let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if val.is_empty() {
            return Err(SecretError::NotFound(name.to_string()));
        }

        Ok(Zeroizing::new(val.into_bytes()))
    }
}
