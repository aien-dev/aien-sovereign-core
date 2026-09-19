use std::process::Command;

pub fn get_vault_secret(key: &str) -> Option<String> {
    if let Ok(val) = std::env::var(key) {
        if !val.trim().is_empty() {
            return Some(val.trim().to_string());
        }
    }

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let vault_candidate = format!("{}/.local/bin/atlas-vault", home);
    let bin = if std::path::Path::new(&vault_candidate).is_file() {
        &vault_candidate
    } else {
        "atlas-vault"
    };
    let output = Command::new(bin).arg("get").arg(key).output().ok()?;

    if output.status.success() {
        let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !val.is_empty() {
            return Some(val);
        }
    }
    None
}
