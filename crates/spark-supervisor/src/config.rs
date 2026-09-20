use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_restart")]
    pub restart: String,
    pub health_url: Option<String>,
}

fn default_restart() -> String {
    "always".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct SupervisorConfig {
    #[serde(default)]
    pub services: HashMap<String, ServiceConfig>,
}

impl SupervisorConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let cfg: SupervisorConfig = toml::from_str(&content)?;
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_supervisor_config() {
        let sample = r#"
[services.cortex]
command = "/usr/local/bin/cortex-rs"
args = ["--port", "18080"]
health_url = "http://127.0.0.1:18080/health"

[services.cockpit]
command = "/usr/local/bin/spark-cockpit-rs"
restart = "on-failure"
"#;
        let cfg: SupervisorConfig = toml::from_str(sample).expect("valid toml");
        assert_eq!(cfg.services.len(), 2);
        let cortex = cfg.services.get("cortex").unwrap();
        assert_eq!(cortex.command, "/usr/local/bin/cortex-rs");
        assert_eq!(cortex.args, vec!["--port", "18080"]);
        assert_eq!(cortex.restart, "always");
        assert_eq!(
            cortex.health_url.as_deref(),
            Some("http://127.0.0.1:18080/health")
        );

        let cockpit = cfg.services.get("cockpit").unwrap();
        assert_eq!(cockpit.restart, "on-failure");
        assert!(cockpit.health_url.is_none());
    }

    #[test]
    fn test_empty_config_defaults() {
        let sample = "";
        let cfg: SupervisorConfig = toml::from_str(sample).expect("empty toml is valid");
        assert!(cfg.services.is_empty());
    }
}
