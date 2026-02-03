use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use crate::model::{Droplet, Image, Region, Size, Snapshot, SshKey};

#[derive(Debug, Deserialize)]
struct DropletApi {
    id: u64,
    name: String,
    status: String,
    region: RegionApi,
    size_slug: Option<String>,
    created_at: Option<String>,
    tags: Option<Vec<String>>,
    networks: Option<NetworksApi>,
}

#[derive(Debug, Deserialize)]
struct RegionApi {
    slug: String,
}

#[derive(Debug, Deserialize)]
struct NetworksApi {
    v4: Vec<NetworkV4>,
}

#[derive(Debug, Deserialize)]
struct NetworkV4 {
    ip_address: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct SnapshotApi {
    id: u64,
    name: String,
    created_at: String,
    regions: Vec<String>,
    resource_id: u64,
    min_disk_size: u64,
    size_gigabytes: f64,
}


#[derive(Debug, Deserialize)]
struct SizeListApi {
    slug: String,
    memory: u64,
    vcpus: u64,
    disk: u64,
    price_monthly: f64,
}

#[derive(Debug, Deserialize)]
struct ImageApi {
    id: u64,
    name: String,
    slug: Option<String>,
    distribution: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SshKeyApi {
    id: u64,
    name: String,
    fingerprint: String,
}

pub fn check_doctl() -> Result<()> {
    let output = Command::new("doctl")
        .args(["account", "get", "-o", "json"])
        .output()
        .context("Failed to execute doctl")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "doctl is not authenticated or failed to run: {stderr}"
        ));
    }
    Ok(())
}

pub fn list_droplets() -> Result<Vec<Droplet>> {
    let raw = run_doctl_json(&["compute", "droplet", "list"])?;
    let api: Vec<DropletApi> = serde_json::from_value(raw)?;
    Ok(api.into_iter().map(map_droplet).collect())
}

pub fn list_snapshots() -> Result<Vec<Snapshot>> {
    let raw = run_doctl_json(&[
        "compute",
        "snapshot",
        "list",
        "--resource",
        "droplet",
    ])?;
    let api: Vec<SnapshotApi> = serde_json::from_value(raw)?;
    Ok(api
        .into_iter()
        .map(|snap| Snapshot {
            id: snap.id,
            name: snap.name,
            created_at: snap.created_at,
            regions: snap.regions,
            resource_id: snap.resource_id,
            min_disk_size: snap.min_disk_size,
            size_gigabytes: snap.size_gigabytes,
        })
        .collect())
}

pub fn list_regions() -> Result<Vec<Region>> {
    Ok(vec![
        Region {
            slug: "nyc1".to_string(),
            name: "New York 1".to_string(),
            available: true,
        },
        Region {
            slug: "sfo1".to_string(),
            name: "San Francisco 1".to_string(),
            available: false,
        },
        Region {
            slug: "nyc2".to_string(),
            name: "New York 2".to_string(),
            available: true,
        },
        Region {
            slug: "ams2".to_string(),
            name: "Amsterdam 2".to_string(),
            available: false,
        },
        Region {
            slug: "sgp1".to_string(),
            name: "Singapore 1".to_string(),
            available: true,
        },
        Region {
            slug: "lon1".to_string(),
            name: "London 1".to_string(),
            available: true,
        },
        Region {
            slug: "nyc3".to_string(),
            name: "New York 3".to_string(),
            available: true,
        },
        Region {
            slug: "ams3".to_string(),
            name: "Amsterdam 3".to_string(),
            available: true,
        },
        Region {
            slug: "fra1".to_string(),
            name: "Frankfurt 1".to_string(),
            available: true,
        },
        Region {
            slug: "tor1".to_string(),
            name: "Toronto 1".to_string(),
            available: true,
        },
        Region {
            slug: "sfo2".to_string(),
            name: "San Francisco 2".to_string(),
            available: true,
        },
        Region {
            slug: "blr1".to_string(),
            name: "Bangalore 1".to_string(),
            available: true,
        },
        Region {
            slug: "sfo3".to_string(),
            name: "San Francisco 3".to_string(),
            available: true,
        },
        Region {
            slug: "syd1".to_string(),
            name: "Sydney 1".to_string(),
            available: true,
        },
        Region {
            slug: "atl1".to_string(),
            name: "Atlanta 1".to_string(),
            available: true,
        },
    ])
}

pub fn list_sizes() -> Result<Vec<Size>> {
    let raw = run_doctl_json(&["compute", "size", "list"])?;
    let api: Vec<SizeListApi> = serde_json::from_value(raw)?;
    Ok(api
        .into_iter()
        .map(|size| Size {
            slug: size.slug,
            memory_mb: size.memory,
            vcpus: size.vcpus,
            disk_gb: size.disk,
            price_monthly: size.price_monthly,
        })
        .collect())
}

pub fn list_images() -> Result<Vec<Image>> {
    let raw = run_doctl_json(&["compute", "image", "list-distribution"])?;
    let api: Vec<ImageApi> = serde_json::from_value(raw)?;
    Ok(api
        .into_iter()
        .map(|image| Image {
            id: image.id,
            name: image.name,
            slug: image.slug,
            distribution: image.distribution,
        })
        .collect())
}

pub fn list_ssh_keys() -> Result<Vec<SshKey>> {
    let raw = run_doctl_json(&["compute", "ssh-key", "list"])?;
    let api: Vec<SshKeyApi> = serde_json::from_value(raw)?;
    Ok(api
        .into_iter()
        .map(|key| SshKey {
            id: key.id,
            name: key.name,
            fingerprint: key.fingerprint,
        })
        .collect())
}

pub fn create_droplet(args: &CreateDropletArgs) -> Result<Droplet> {
    let raw = run_doctl_json_owned(build_create_command(args))?;
    let api: Vec<DropletApi> = serde_json::from_value(raw)?;
    let droplet = api
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("No droplet returned from create"))?;
    Ok(map_droplet(droplet))
}

pub fn create_droplet_from_snapshot(args: &CreateDropletArgs) -> Result<Droplet> {
    create_droplet(args)
}

fn build_create_command(args: &CreateDropletArgs) -> Vec<String> {
    let mut cmd = vec![
        "compute".to_string(),
        "droplet".to_string(),
        "create".to_string(),
        args.name.clone(),
        "--size".to_string(),
        args.size.clone(),
        "--image".to_string(),
        args.image.clone(),
        "--wait".to_string(),
    ];

    if let Some(region) = args.region.as_ref() {
        if !region.trim().is_empty() {
            cmd.push("--region".to_string());
            cmd.push(region.clone());
        }
    }

    if !args.ssh_keys.is_empty() {
        cmd.push("--ssh-keys".to_string());
        cmd.push(args.ssh_keys.join(","));
    }

    if !args.tags.is_empty() {
        cmd.push("--tag-names".to_string());
        cmd.push(args.tags.join(","));
    }

    cmd
}

pub fn snapshot_droplet(droplet_id: u64, snapshot_name: &str) -> Result<()> {
    let cmd = vec![
        "compute".to_string(),
        "droplet-action".to_string(),
        "snapshot".to_string(),
        droplet_id.to_string(),
        "--snapshot-name".to_string(),
        snapshot_name.to_string(),
        "--wait".to_string(),
    ];
    run_doctl_json_owned(cmd)?;
    Ok(())
}

pub fn delete_droplet(droplet_id: u64) -> Result<()> {
    let output = Command::new("doctl")
        .args([
            "compute",
            "droplet",
            "delete",
            &droplet_id.to_string(),
            "--force",
        ])
        .output()
        .context("Failed to execute doctl delete")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("doctl delete failed: {stderr}"));
    }
    Ok(())
}

fn map_droplet(droplet: DropletApi) -> Droplet {
    let (public_ipv4, private_ipv4) = droplet
        .networks
        .as_ref()
        .map(|networks| {
            let mut public_ip = None;
            let mut private_ip = None;
            for net in &networks.v4 {
                if net.kind == "public" {
                    public_ip = Some(net.ip_address.clone());
                } else if net.kind == "private" {
                    private_ip = Some(net.ip_address.clone());
                }
            }
            (public_ip, private_ip)
        })
        .unwrap_or((None, None));

    Droplet {
        id: droplet.id,
        name: droplet.name,
        status: droplet.status,
        region: droplet.region.slug,
        size: droplet.size_slug,
        public_ipv4,
        private_ipv4,
        created_at: droplet.created_at,
        tags: droplet.tags.unwrap_or_default(),
    }
}

fn run_doctl_json(args: &[&str]) -> Result<serde_json::Value> {
    let output = Command::new("doctl")
        .args(args)
        .args(["-o", "json"])
        .output()
        .context("Failed to execute doctl")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("doctl failed: {stderr}"));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).context("Failed to parse doctl JSON output")
}

fn run_doctl_json_owned(args: Vec<String>) -> Result<serde_json::Value> {
    let output = Command::new("doctl")
        .args(args)
        .args(["-o", "json"])
        .output()
        .context("Failed to execute doctl")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("doctl failed: {stderr}"));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).context("Failed to parse doctl JSON output")
}

#[derive(Debug, Clone)]
pub struct CreateDropletArgs {
    pub name: String,
    pub region: Option<String>,
    pub size: String,
    pub image: String,
    pub ssh_keys: Vec<String>,
    pub tags: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_droplet_picks_public_and_private_ips() {
        let api = DropletApi {
            id: 42,
            name: "test".to_string(),
            status: "active".to_string(),
            region: RegionApi {
                slug: "nyc1".to_string(),
            },
            size_slug: Some("s-1vcpu-1gb".to_string()),
            created_at: Some("2024-01-01T00:00:00Z".to_string()),
            tags: None,
            networks: Some(NetworksApi {
                v4: vec![
                    NetworkV4 {
                        ip_address: "10.0.0.2".to_string(),
                        kind: "private".to_string(),
                    },
                    NetworkV4 {
                        ip_address: "203.0.113.10".to_string(),
                        kind: "public".to_string(),
                    },
                ],
            }),
        };
        let droplet = map_droplet(api);
        assert_eq!(droplet.public_ipv4.as_deref(), Some("203.0.113.10"));
        assert_eq!(droplet.private_ipv4.as_deref(), Some("10.0.0.2"));
        assert_eq!(droplet.tags.len(), 0);
    }

    #[test]
    fn build_create_command_includes_optional_fields() {
        let args = CreateDropletArgs {
            name: "demo".to_string(),
            region: Some("nyc1".to_string()),
            size: "s-1vcpu-1gb".to_string(),
            image: "ubuntu-22-04-x64".to_string(),
            ssh_keys: vec!["123".to_string(), "456".to_string()],
            tags: vec!["dev".to_string(), "test".to_string()],
        };
        let cmd = build_create_command(&args);
        let joined = cmd.join(" ");
        assert!(joined.contains("compute droplet create demo"));
        assert!(joined.contains("--region nyc1"));
        assert!(joined.contains("--ssh-keys 123,456"));
        assert!(joined.contains("--tag-names dev,test"));
    }

    #[test]
    fn build_create_command_omits_empty_optionals() {
        let args = CreateDropletArgs {
            name: "demo".to_string(),
            region: Some("".to_string()),
            size: "s-1vcpu-1gb".to_string(),
            image: "ubuntu-22-04-x64".to_string(),
            ssh_keys: vec![],
            tags: vec![],
        };
        let cmd = build_create_command(&args);
        let joined = cmd.join(" ");
        assert!(!joined.contains("--region"));
        assert!(!joined.contains("--ssh-keys"));
        assert!(!joined.contains("--tag-names"));
    }

    #[test]
    fn list_regions_returns_hardcoded_list() {
        let regions = list_regions().expect("regions");
        assert_eq!(regions.len(), 15);
        let nyc1 = regions.iter().find(|r| r.slug == "nyc1").unwrap();
        assert!(nyc1.available);
        let sfo1 = regions.iter().find(|r| r.slug == "sfo1").unwrap();
        assert!(!sfo1.available);
    }
}
