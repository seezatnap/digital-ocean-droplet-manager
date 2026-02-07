use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossbeam_channel::Sender;

use crate::doctl::{self, CreateDropletArgs};
use crate::model::{Droplet, Image, PortBinding, Region, Size, Snapshot, SshKey};
use crate::mutagen::{self, DeleteSyncOutcome, SshConfig, SyncPath, SyncSession};
use crate::ports;

#[derive(Debug, Clone)]
pub enum Task {
    CheckDoctl,
    RefreshDroplets,
    LoadSnapshots,
    LoadSnapshotsDelayed { delay_ms: u64 },
    LoadRegions,
    LoadSizes,
    LoadImages,
    LoadSshKeys,
    CreateDroplet(CreateDropletArgs),
    RestoreDroplet(CreateDropletArgs),
    SnapshotDelete { droplet_id: u64, snapshot_name: String },
    DeleteDroplet { droplet_id: u64 },
    StartTunnel(PortBinding),
    StopTunnel { port: u16, pid: u32 },
    CreateSyncs {
        ssh: SshConfig,
        droplet_name: String,
        paths: Vec<SyncPath>,
    },
    RestoreSyncs { ssh: SshConfig },
    LoadSyncs,
    DeleteSync {
        name: String,
        ssh: Option<SshConfig>,
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
            Task::TerminateAllSyncs => {
                TaskResult::TerminateAllSyncs(mutagen::terminate_all_syncs())
            }
        };
        let _ = tx.send(result);
    });
}
