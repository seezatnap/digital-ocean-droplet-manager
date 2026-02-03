use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Droplet {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub region: String,
    pub size: Option<String>,
    pub public_ipv4: Option<String>,
    pub private_ipv4: Option<String>,
    pub created_at: Option<String>,
    pub tags: Vec<String>,
}

impl Droplet {
    pub fn is_running(&self) -> bool {
        self.status == "active"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: u64,
    pub name: String,
    pub created_at: String,
    pub regions: Vec<String>,
    pub resource_id: u64,
    pub min_disk_size: u64,
    pub size_gigabytes: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Region {
    pub slug: String,
    pub name: String,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Size {
    pub slug: String,
    pub memory_mb: u64,
    pub vcpus: u64,
    pub disk_gb: u64,
    pub price_monthly: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Image {
    pub id: u64,
    pub name: String,
    pub slug: Option<String>,
    pub distribution: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshKey {
    pub id: u64,
    pub name: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortBinding {
    pub droplet_id: u64,
    pub droplet_name: String,
    pub public_ip: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub ssh_user: String,
    pub ssh_key_path: String,
    pub ssh_port: u16,
    pub created_at: DateTime<Utc>,
    pub tunnel_pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    pub default_ssh_user: String,
    pub default_ssh_key_path: String,
    pub default_ssh_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppStateFile {
    pub bindings: Vec<PortBinding>,
    pub settings: Settings,
}
