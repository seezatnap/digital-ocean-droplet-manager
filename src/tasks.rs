use std::thread;
use std::time::Duration;
use std::{path::Path, path::PathBuf, process::Command};

use anyhow::{Context, Result, anyhow};
use crossbeam_channel::Sender;

use crate::doctl::{self, CreateDropletArgs};
use crate::model::{Droplet, Image, PortBinding, Region, Size, Snapshot, SshKey};
use crate::mutagen::{
    self, DeleteDropletSyncsOutcome, DeleteSyncOutcome, SshConfig, SyncPath, SyncSession,
};
use crate::ports;

#[derive(Debug, Clone)]
pub struct RemoteDirectoryListing {
    pub path: String,
    pub directories: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Task {
    CheckDoctl,
    RefreshDroplets,
    LoadSnapshots,
    LoadSnapshotsDelayed {
        delay_ms: u64,
    },
    LoadRegions,
    LoadSizes,
    LoadImages,
    LoadSshKeys,
    CreateDroplet(CreateDropletArgs),
    RestoreDroplet(CreateDropletArgs),
    SnapshotDelete {
        droplet_id: u64,
        snapshot_name: String,
    },
    DeleteDroplet {
        droplet_id: u64,
    },
    StartTunnel(PortBinding),
    StopTunnel {
        port: u16,
        pid: u32,
    },
    CreateSyncs {
        ssh: SshConfig,
        droplet_name: String,
        paths: Vec<SyncPath>,
    },
    RestoreSyncs {
        ssh: SshConfig,
    },
    LoadSyncs,
    DeleteSync {
        name: String,
        ssh: Option<SshConfig>,
    },
    ListRemoteDirectories {
        ssh: SshConfig,
        path: String,
    },
    DeleteDropletSyncs {
        ssh: SshConfig,
        droplet_name: String,
    },
    TerminateAllSyncs,
}

#[derive(Debug)]
pub enum TaskResult {
    DoctlCheck(Result<()>),
    Droplets(Result<Vec<Droplet>>),
    Snapshots(Result<Vec<Snapshot>>),
    Regions(Result<Vec<Region>>),
    Sizes(Result<Vec<Size>>),
    Images(Result<Vec<Image>>),
    SshKeys(Result<Vec<SshKey>>),
    CreateDroplet(Result<Droplet>),
    RestoreDroplet(Result<Droplet>),
    SnapshotDelete(Result<()>),
    DeleteDroplet(Result<()>),
    StartTunnel(Result<PortBinding>),
    StopTunnel(Result<u16>),
    CreateSyncs(Result<usize>),
    RestoreSyncs(Result<usize>),
    Syncs(Result<Vec<SyncSession>>),
    DeleteSync(Result<DeleteSyncOutcome>),
    RemoteDirectories {
        requested_path: String,
        result: Result<RemoteDirectoryListing>,
    },
    DeleteDropletSyncs(Result<DeleteDropletSyncsOutcome>),
    TerminateAllSyncs(Result<usize>),
}

pub fn spawn(task: Task, tx: Sender<TaskResult>) {
    thread::spawn(move || {
        let result = match task {
            Task::CheckDoctl => TaskResult::DoctlCheck(doctl::check_doctl()),
            Task::RefreshDroplets => TaskResult::Droplets(doctl::list_droplets()),
            Task::LoadSnapshots => TaskResult::Snapshots(doctl::list_snapshots()),
            Task::LoadSnapshotsDelayed { delay_ms } => {
                thread::sleep(Duration::from_millis(delay_ms));
                TaskResult::Snapshots(doctl::list_snapshots())
            }
            Task::LoadRegions => TaskResult::Regions(doctl::list_regions()),
            Task::LoadSizes => TaskResult::Sizes(doctl::list_sizes()),
            Task::LoadImages => TaskResult::Images(doctl::list_images()),
            Task::LoadSshKeys => TaskResult::SshKeys(doctl::list_ssh_keys()),
            Task::CreateDroplet(args) => TaskResult::CreateDroplet(doctl::create_droplet(&args)),
            Task::RestoreDroplet(args) => {
                TaskResult::RestoreDroplet(doctl::create_droplet_from_snapshot(&args))
            }
            Task::SnapshotDelete {
                droplet_id,
                snapshot_name,
            } => TaskResult::SnapshotDelete(
                doctl::snapshot_droplet(droplet_id, &snapshot_name)
                    .and_then(|_| doctl::delete_droplet(droplet_id)),
            ),
            Task::DeleteDroplet { droplet_id } => {
                TaskResult::DeleteDroplet(doctl::delete_droplet(droplet_id))
            }
            Task::StartTunnel(mut binding) => {
                let res = ports::start_tunnel(&mut binding).map(|_| binding);
                TaskResult::StartTunnel(res)
            }
            Task::StopTunnel { port, pid } => {
                let res = ports::stop_tunnel(pid).map(|_| port);
                TaskResult::StopTunnel(res)
            }
            Task::CreateSyncs {
                ssh,
                droplet_name,
                paths,
            } => TaskResult::CreateSyncs(mutagen::create_syncs(&ssh, &droplet_name, paths)),
            Task::RestoreSyncs { ssh } => TaskResult::RestoreSyncs(mutagen::restore_syncs(&ssh)),
            Task::LoadSyncs => TaskResult::Syncs(mutagen::list_syncs()),
            Task::DeleteSync { name, ssh } => {
                TaskResult::DeleteSync(mutagen::delete_sync(&name, ssh.as_ref()))
            }
            Task::ListRemoteDirectories { ssh, path } => TaskResult::RemoteDirectories {
                requested_path: path.clone(),
                result: list_remote_directories(&ssh, &path),
            },
            Task::DeleteDropletSyncs { ssh, droplet_name } => TaskResult::DeleteDropletSyncs(
                mutagen::delete_syncs_for_droplet(&ssh, &droplet_name),
            ),
            Task::TerminateAllSyncs => {
                TaskResult::TerminateAllSyncs(mutagen::terminate_all_syncs())
            }
        };
        let _ = tx.send(result);
    });
}

fn list_remote_directories(ssh: &SshConfig, path: &str) -> Result<RemoteDirectoryListing> {
    let key_path = expand_local_path(&ssh.key_path);
    let remote_cmd = format!(
        "TARGET={}; \
         if [ \"$TARGET\" = \"~\" ]; then TARGET=\"$HOME\"; fi; \
         cd -- \"$TARGET\" 2>/dev/null || exit 2; \
         pwd; \
         ls -1Ap 2>/dev/null | sed -n 's:/$::p' | LC_ALL=C sort",
        shell_escape(path)
    );

    let output = Command::new("ssh")
        .arg("-i")
        .arg(&key_path)
        .arg("-p")
        .arg(ssh.port.to_string())
        .arg("-o")
        .arg("BatchMode=yes")
        .arg(format!("{}@{}", ssh.user, ssh.host))
        .arg(remote_cmd)
        .output()
        .context("Failed to execute ssh")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ssh failed: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let resolved = lines
        .next()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .ok_or_else(|| anyhow!("Remote directory listing returned no path"))?;

    let mut directories = Vec::new();
    for line in lines {
        let name = line.trim_end_matches('\r');
        if !name.is_empty() {
            directories.push(name.to_string());
        }
    }

    Ok(RemoteDirectoryListing {
        path: resolved.to_string(),
        directories,
    })
}

fn expand_local_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed == "~" || trimmed.starts_with("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
        if trimmed == "~" {
            return home;
        }
        return format!("{home}{}", &trimmed[1..]);
    }
    let p = Path::new(trimmed);
    if p.is_absolute() {
        return trimmed.to_string();
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.join(p).to_string_lossy().to_string()
}

fn shell_escape(value: &str) -> String {
    if value.is_empty() {
        "''".to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}
