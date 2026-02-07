use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;

use crate::model::{AppStateFile, PortBinding};

pub fn is_port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

pub fn port_in_registry(state: &AppStateFile, port: u16) -> Option<&PortBinding> {
    state
        .bindings
        .iter()
        .find(|binding| binding.local_port == port)
}

pub fn start_tunnel(binding: &mut PortBinding) -> Result<u32> {
    let mut child = spawn_ssh_tunnel(binding)?;
    std::thread::sleep(Duration::from_millis(250));
    match child.try_wait() {
        Ok(Some(status)) => {
            let stderr = read_child_stderr(&mut child);
            return Err(anyhow!("SSH tunnel exited early ({status}). {stderr}"));
        }
        Ok(None) => {
            let pid = child.id();
            binding.tunnel_pid = Some(pid);
            Ok(pid)
        }
        Err(err) => Err(anyhow!("Failed to poll SSH tunnel: {err}")),
    }
}

pub fn spawn_ssh_tunnel(binding: &PortBinding) -> Result<Child> {
    let mut cmd = Command::new("ssh");
    cmd.arg("-N")
        .arg("-L")
        .arg(format!(
            "127.0.0.1:{}:127.0.0.1:{}",
            binding.local_port, binding.remote_port
        ))
        .arg("-o")
        .arg("ExitOnForwardFailure=yes")
        .arg("-o")
        .arg("ServerAliveInterval=30")
        .arg("-o")
        .arg("ServerAliveCountMax=3")
        .arg("-i")
        .arg(&binding.ssh_key_path)
        .arg("-p")
        .arg(binding.ssh_port.to_string())
        .arg(format!("{}@{}", binding.ssh_user, binding.public_ip))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    cmd.spawn().context("Failed to start SSH tunnel")
}

pub fn read_child_stderr(child: &mut Child) -> String {
    if let Some(stderr) = child.stderr.take() {
        let mut reader = std::io::BufReader::new(stderr);
        let mut out = String::new();
        let _ = std::io::Read::read_to_string(&mut reader, &mut out);
        return out;
    }
    String::new()
}

pub fn is_pid_running(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

pub fn stop_tunnel(pid: u32) -> Result<()> {
    let res = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    if res != 0 {
        return Err(anyhow!("Failed to send SIGTERM to PID {pid}"));
    }
    Ok(())
}

pub fn new_binding(
    droplet_id: u64,
    droplet_name: String,
    public_ip: String,
    local_port: u16,
    remote_port: u16,
    ssh_user: String,
    ssh_key_path: String,
    ssh_port: u16,
) -> PortBinding {
    PortBinding {
        droplet_id,
        droplet_name,
        public_ip,
        local_port,
        remote_port,
        ssh_user,
        ssh_key_path,
        ssh_port,
        created_at: Utc::now(),
        tunnel_pid: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AppStateFile;
    use std::net::TcpListener;

    #[test]
    fn registry_lookup_matches_local_port() {
        let binding = new_binding(
            1,
            "droplet".to_string(),
            "127.0.0.1".to_string(),
            8080,
            80,
            "root".to_string(),
            "/tmp/id_rsa".to_string(),
            22,
        );
        let state = AppStateFile {
            bindings: vec![binding],
            settings: Default::default(),
        };
        assert!(port_in_registry(&state, 8080).is_some());
        assert!(port_in_registry(&state, 9090).is_none());
    }

    #[test]
    fn port_availability_detects_in_use() {
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => listener,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                return;
            }
            Err(err) => panic!("bind failed: {err}"),
        };
        let port = listener.local_addr().unwrap().port();
        assert!(!is_port_available(port));
        drop(listener);
    }
}
