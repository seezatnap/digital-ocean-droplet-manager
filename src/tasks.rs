use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use crossbeam_channel::Sender;

use crate::doctl::{self, CreateDropletArgs};
use crate::model::{Droplet, Image, PortBinding, Region, RsyncBind, Size, Snapshot, SshKey};
use crate::mutagen::{
    self, DeleteDropletSyncsOutcome, DeleteSyncOutcome, SshConfig, SyncPath, SyncSession,
};
use crate::ports;

#[derive(Debug, Clone)]
pub struct RemoteDirectoryListing {
    pub path: String,
    pub directories: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RsyncDirection {
    Up,
    Down,
}

#[derive(Debug, Clone)]
pub struct RsyncRunOutcome {
    pub bind: RsyncBind,
    pub direction: RsyncDirection,
}

#[derive(Debug, Clone)]
pub struct DeleteRsyncBindOutcome {
    pub bind: RsyncBind,
    pub local_deleted: bool,
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
    CreateRsyncBind {
        bind: RsyncBind,
    },
    RunRsync {
        bind: RsyncBind,
        direction: RsyncDirection,
    },
    DeleteRsyncBind {
        bind: RsyncBind,
        delete_local_copy: bool,
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
    CreateRsyncBind(Result<RsyncBind>),
    RunRsync(Result<RsyncRunOutcome>),
    DeleteRsyncBind(Result<DeleteRsyncBindOutcome>),
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
            Task::CreateRsyncBind { bind } => TaskResult::CreateRsyncBind(create_rsync_bind(&bind)),
            Task::RunRsync { bind, direction } => TaskResult::RunRsync(run_rsync(&bind, direction)),
            Task::DeleteRsyncBind {
                bind,
                delete_local_copy,
            } => TaskResult::DeleteRsyncBind(delete_rsync_bind(bind, delete_local_copy)),
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

fn create_rsync_bind(bind: &RsyncBind) -> Result<RsyncBind> {
    let local_path = expand_local_path(&bind.local_path);
    let local = Path::new(&local_path);
    if local.exists() {
        let metadata = fs::metadata(local)
            .with_context(|| format!("Failed to inspect local path '{local_path}'"))?;
        if !metadata.is_dir() {
            return Err(anyhow!(
                "Local path '{}' exists and is not a directory. Pick a different local folder.",
                local_path
            ));
        }
        if !is_dir_empty(local)? {
            return Err(anyhow!(
                "Local folder '{}' is not empty. Move/remove its contents or pick a different folder.",
                local_path
            ));
        }
    } else {
        fs::create_dir_all(local)
            .with_context(|| format!("Failed to create local folder '{}'", local_path))?;
    }

    let mut created = bind.clone();
    created.local_path = local_path;
    Ok(created)
}

fn run_rsync(bind: &RsyncBind, direction: RsyncDirection) -> Result<RsyncRunOutcome> {
    let local_path = expand_local_path(&bind.local_path);
    fs::create_dir_all(&local_path)
        .with_context(|| format!("Failed to ensure local folder '{local_path}'"))?;

    let key_path = expand_local_path(&bind.ssh_key_path);
    let remote = format!("{}@{}:{}", bind.ssh_user, bind.host, bind.remote_path);
    let ssh_cmd = format!(
        "ssh -i {} -p {} -o BatchMode=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=3",
        shell_escape_arg(&key_path),
        bind.ssh_port
    );

    let (source, dest) = match direction {
        RsyncDirection::Up => (format!("{}/", local_path), remote),
        RsyncDirection::Down => (format!("{remote}/"), format!("{}/", local_path)),
    };

    let output = Command::new("rsync")
        .arg("-az")
        .arg("--human-readable")
        .arg("--exclude=node_modules")
        .arg("--exclude=target")
        .arg("--exclude=/.cargo*")
        .arg("-e")
        .arg(ssh_cmd)
        .arg(source)
        .arg(dest)
        .output()
        .context("Failed to execute rsync")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(anyhow!(
            "rsync failed ({:?}).\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            if stdout.is_empty() {
                "<empty>"
            } else {
                &stdout
            },
            if stderr.is_empty() {
                "<empty>"
            } else {
                &stderr
            }
        ));
    }

    let mut result_bind = bind.clone();
    result_bind.local_path = local_path;
    Ok(RsyncRunOutcome {
        bind: result_bind,
        direction,
    })
}

fn delete_rsync_bind(bind: RsyncBind, delete_local_copy: bool) -> Result<DeleteRsyncBindOutcome> {
    let local_path = expand_local_path(&bind.local_path);
    let mut local_deleted = false;
    if delete_local_copy {
        let path = Path::new(&local_path);
        if path.exists() {
            if path.is_dir() {
                fs::remove_dir_all(path)
                    .with_context(|| format!("Failed to remove local folder '{local_path}'"))?;
            } else {
                fs::remove_file(path)
                    .with_context(|| format!("Failed to remove local file '{local_path}'"))?;
            }
            local_deleted = true;
        }
    }
    Ok(DeleteRsyncBindOutcome {
        bind,
        local_deleted,
    })
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

fn is_dir_empty(path: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("Failed to read directory '{}'", path.display()))?;
    Ok(entries.next().is_none())
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

fn shell_escape_arg(value: &str) -> String {
    if value.is_empty() {
        "''".to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}
