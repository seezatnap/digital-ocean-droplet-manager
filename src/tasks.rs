use std::thread;

use anyhow::Result;
use crossbeam_channel::Sender;

use crate::doctl::{self, CreateDropletArgs};
use crate::model::{Droplet, Image, PortBinding, Region, Size, Snapshot, SshKey};
use crate::ports;

#[derive(Debug, Clone)]
pub enum Task {
    CheckDoctl,
    RefreshDroplets,
    LoadSnapshots,
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
}

pub fn spawn(task: Task, tx: Sender<TaskResult>) {
    thread::spawn(move || {
        let result = match task {
            Task::CheckDoctl => TaskResult::DoctlCheck(doctl::check_doctl()),
            Task::RefreshDroplets => TaskResult::Droplets(doctl::list_droplets()),
            Task::LoadSnapshots => TaskResult::Snapshots(doctl::list_snapshots()),
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
        };
        let _ = tx.send(result);
    });
}
