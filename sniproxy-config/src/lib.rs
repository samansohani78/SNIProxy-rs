use std::path::Path;
use std::fs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub listen_addrs: Vec<String>,
    pub timeouts: Timeouts,
    pub metrics: Metrics,
    pub allowlist: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Timeouts {
    pub connect: u64,
    pub client_hello: u64,
    pub idle: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metrics {
    pub enabled: bool,
    pub address: String,
}

impl Config {
    pub fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let config = serde_yaml::from_str(&contents)?;
        Ok(config)
    }
}
