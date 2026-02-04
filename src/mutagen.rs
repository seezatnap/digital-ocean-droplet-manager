use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;

#[derive(Debug, Clone)]
pub struct SyncPath {
    pub local: String,
    pub remote: String,
}

#[derive(Debug, Clone)]
pub struct SshConfig {
    pub user: String,
    pub host: String,
    pub port: u16,
    pub key_path: String,
}

#[derive(Debug, Clone)]
pub struct SyncSession {
    pub name: String,
    pub status: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeleteSyncOutcome {
    pub name: String,
    pub mount_removed: bool,
    pub mount_error: Option<String>,
}

#[derive(Debug, Clone)]
struct MountEntry {
    name: String,
    local: String,
    remote: String,
}

pub fn create_syncs(ssh: &SshConfig, droplet_name: &str, paths: Vec<SyncPath>) -> Result<usize> {
    if paths.is_empty() {
        return Err(anyhow!("No folders provided for sync"));
    }

    let mut existing_entries = read_mountlist(ssh)?;
    let mut existing_names = mutagen_existing_names()?;
    let mut new_entries = Vec::new();
    let mut created = 0usize;

    let mut seen_pairs = HashSet::new();
    let mut index = 1usize;

    for path in paths {
        let local = expand_local_path(&path.local);
        let remote = path.remote.trim().to_string();
        if remote.is_empty() {
            return Err(anyhow!("Remote path cannot be empty"));
        }

        let key = format!("{}\n{}", local, remote);
        if !seen_pairs.insert(key) {
            continue;
        }

        let name = match existing_entries
            .iter()
            .find(|entry| entry.local == local && entry.remote == remote)
        {
            Some(entry) => entry.name.clone(),
            None => {
                let name = generate_sync_name(droplet_name, &local, index);
                index += 1;
                let entry = MountEntry {
                    name: name.clone(),
                    local: local.clone(),
                    remote: remote.clone(),
                };
                existing_entries.push(entry.clone());
                new_entries.push(entry);
                name
            }
        };

        ensure_remote_dir(ssh, &remote)?;
        if existing_names.contains(&name) {
            mutagen_resume(&name)?;
        } else {
            mutagen_create(ssh, &name, &local, &remote)?;
            existing_names.insert(name);
        }
        created += 1;
    }

    if !new_entries.is_empty() {
        append_mountlist(ssh, &new_entries)?;
    }

    Ok(created)
}

pub fn restore_syncs(ssh: &SshConfig) -> Result<usize> {
    let entries = read_mountlist(ssh)?;
    if entries.is_empty() {
        return Err(anyhow!("No mounts found in ~/.mountlist"));
    }

    let mut existing_names = mutagen_existing_names()?;
    let mut restored = 0usize;

    for entry in entries {
        let local = expand_local_path(&entry.local);
        ensure_remote_dir(ssh, &entry.remote)?;
        if existing_names.contains(&entry.name) {
            mutagen_resume(&entry.name)?;
        } else {
            mutagen_create(ssh, &entry.name, &local, &entry.remote)?;
            existing_names.insert(entry.name);
        }
        restored += 1;
    }

    Ok(restored)
}

pub fn list_syncs() -> Result<Vec<SyncSession>> {
    if let Ok(output) = run_mutagen(&["sync", "list", "--json"]) {
        if let Ok(sessions) = sessions_from_json(&output) {
            if !sessions.is_empty() {
                return Ok(sessions);
            }
        }
    }

    let output = run_mutagen(&["sync", "list"])?;
    Ok(sessions_from_text(&output))
}

pub fn terminate_sync(name: &str) -> Result<()> {
    run_mutagen(&["sync", "terminate", name])?;
    Ok(())
}

pub fn delete_sync(name: &str, ssh: Option<&SshConfig>) -> Result<DeleteSyncOutcome> {
    terminate_sync(name)?;
    let mut mount_removed = false;
    let mut mount_error = None;
    if let Some(ssh) = ssh {
        match delete_mount_entries(ssh, &[name.to_string()]) {
            Ok(count) => {
                mount_removed = count > 0;
            }
            Err(err) => {
                mount_error = Some(err.to_string());
            }
        }
    }
    Ok(DeleteSyncOutcome {
        name: name.to_string(),
        mount_removed,
        mount_error,
    })
}

fn mutagen_existing_names() -> Result<HashSet<String>> {
    if let Ok(output) = run_mutagen(&["sync", "list", "--json"]) {
        if let Ok(names) = names_from_json(&output) {
            if !names.is_empty() {
                return Ok(names);
            }
        }
    }

    let output = run_mutagen(&["sync", "list"])?;
    let sessions = sessions_from_text(&output);
    Ok(sessions.into_iter().map(|s| s.name).collect())
}

fn mutagen_create(ssh: &SshConfig, name: &str, local: &str, remote: &str) -> Result<()> {
    let remote_target = format!("{}@{}:{}", ssh.user, ssh.host, remote);
    run_mutagen(&[
        "sync",
        "create",
        "--name",
        name,
        local,
        &remote_target,
    ])?;
    Ok(())
}

fn mutagen_resume(name: &str) -> Result<()> {
    run_mutagen(&["sync", "resume", name])?;
    Ok(())
}

fn run_mutagen(args: &[&str]) -> Result<String> {
    let output = Command::new("mutagen")
        .args(args)
        .output()
        .context("Failed to execute mutagen")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("mutagen failed: {stderr}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn names_from_json(raw: &str) -> Result<HashSet<String>> {
    let value: serde_json::Value =
        serde_json::from_str(raw).context("Failed to parse mutagen JSON")?;
    let mut names = HashSet::new();
    if let Some(array) = value.as_array() {
        for item in array {
            if let Some(name) = item
                .get("name")
                .or_else(|| item.get("Name"))
                .and_then(|v| v.as_str())
            {
                if !name.is_empty() {
                    names.insert(name.to_string());
                }
            }
        }
    }
    Ok(names)
}

fn sessions_from_json(raw: &str) -> Result<Vec<SyncSession>> {
    let value: serde_json::Value =
        serde_json::from_str(raw).context("Failed to parse mutagen JSON")?;
    let mut sessions = Vec::new();
    if let Some(array) = value.as_array() {
        for item in array {
            let name = item
                .get("name")
                .or_else(|| item.get("Name"))
                .and_then(|v| v.as_str());
            if let Some(name) = name {
                let status = item
                    .get("status")
                    .or_else(|| item.get("Status"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                sessions.push(SyncSession {
                    name: name.to_string(),
                    status,
                });
            }
        }
    }
    Ok(sessions)
}

fn sessions_from_text(raw: &str) -> Vec<SyncSession> {
    let mut sessions = Vec::new();
    let mut current: Option<usize> = None;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Name:") {
            let name = rest.trim();
            if !name.is_empty() {
                sessions.push(SyncSession {
                    name: name.to_string(),
                    status: None,
                });
                current = Some(sessions.len() - 1);
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Status:") {
            if let Some(idx) = current {
                let status = rest.trim();
                if !status.is_empty() {
                    sessions[idx].status = Some(status.to_string());
                }
            }
            continue;
        }
        let lower = trimmed.to_lowercase();
        if lower.starts_with("name:") {
            let name = trimmed[5..].trim();
            if !name.is_empty() {
                sessions.push(SyncSession {
                    name: name.to_string(),
                    status: None,
                });
                current = Some(sessions.len() - 1);
            }
            continue;
        }
        if lower.starts_with("status:") {
            if let Some(idx) = current {
                let status = trimmed[7..].trim();
                if !status.is_empty() {
                    sessions[idx].status = Some(status.to_string());
                }
            }
            continue;
        }
    }

    if sessions.is_empty() {
        let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.len() >= 2 {
            for line in lines.iter().skip(1) {
                let cols: Vec<&str> = line.split_whitespace().collect();
                if cols.len() >= 2 {
                    sessions.push(SyncSession {
                        name: cols[1].to_string(),
                        status: None,
                    });
                } else if cols.len() == 1 {
                    let value = cols[0].trim();
                    if !value.is_empty() && !value.ends_with(':') {
                        sessions.push(SyncSession {
                            name: value.to_string(),
                            status: None,
                        });
                    }
                }
            }
        }
    }
    sessions
}

fn read_mountlist(ssh: &SshConfig) -> Result<Vec<MountEntry>> {
    let output = run_ssh(ssh, "cat ~/.mountlist 2>/dev/null || true")?;
    Ok(parse_mountlist(&output))
}

pub fn delete_mount_entries(ssh: &SshConfig, names: &[String]) -> Result<usize> {
    if names.is_empty() {
        return Ok(0);
    }
    let entries = read_mountlist(ssh)?;
    if entries.is_empty() {
        return Ok(0);
    }
    let mut remove = HashSet::new();
    for name in names {
        remove.insert(name.as_str());
    }
    let removed = entries.iter().filter(|entry| remove.contains(entry.name.as_str())).count();
    if removed == 0 {
        return Ok(0);
    }
    let mut script = String::from("if [ -f ~/.mountlist ]; then ");
    script.push_str("awk -F '\\t' 'BEGIN{");
    for name in names {
        script.push_str(&format!("del[\"{}\"]=1;", name));
    }
    script.push_str("} !($1 in del){print}' ~/.mountlist > ~/.mountlist.tmp && mv ~/.mountlist.tmp ~/.mountlist; fi");
    run_ssh(ssh, &script)?;
    Ok(removed)
}

fn append_mountlist(ssh: &SshConfig, entries: &[MountEntry]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let mut lines = String::new();
    for entry in entries {
        lines.push_str(&format!(
            "printf '%s\\t%s\\t%s\\n' {} {} {} >> ~/.mountlist\n",
            shell_escape(&entry.name),
            shell_escape(&entry.local),
            shell_escape(&entry.remote)
        ));
    }
    run_ssh(ssh, &lines)?;
    Ok(())
}

fn ensure_remote_dir(ssh: &SshConfig, remote: &str) -> Result<()> {
    let cmd = format!("mkdir -p {}", remote_path_command(remote));
    run_ssh(ssh, &cmd)?;
    Ok(())
}

fn run_ssh(ssh: &SshConfig, command: &str) -> Result<String> {
    let key_path = expand_local_path(&ssh.key_path);
    let output = Command::new("ssh")
        .arg("-i")
        .arg(&key_path)
        .arg("-p")
        .arg(ssh.port.to_string())
        .arg("-o")
        .arg("BatchMode=yes")
        .arg(format!("{}@{}", ssh.user, ssh.host))
        .arg(command)
        .output()
        .context("Failed to execute ssh")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ssh failed: {stderr}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn parse_mountlist(content: &str) -> Vec<MountEntry> {
    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let name = parts[0].trim();
        let local = parts[1].trim();
        let remote = parts[2].trim();
        if name.is_empty() || local.is_empty() || remote.is_empty() {
            continue;
        }
        entries.push(MountEntry {
            name: name.to_string(),
            local: local.to_string(),
            remote: remote.to_string(),
        });
    }
    entries
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

fn generate_sync_name(droplet_name: &str, local: &str, index: usize) -> String {
    let base = Path::new(local)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("sync");
    let droplet = sanitize_name(droplet_name);
    let base = sanitize_name(base);
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    if index > 1 {
        format!("sync-{}-{}-{}-{}", droplet, base, stamp, index)
    } else {
        format!("sync-{}-{}-{}", droplet, base, stamp)
    }
}

fn sanitize_name(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;
    for ch in input.trim().chars() {
        let next = match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => {
                last_dash = false;
                Some(ch)
            }
            '-' | '_' => {
                last_dash = false;
                Some(ch)
            }
            _ if ch.is_whitespace() || ch == '.' => {
                if last_dash {
                    None
                } else {
                    last_dash = true;
                    Some('-')
                }
            }
            _ => None,
        };
        if let Some(ch) = next {
            out.push(ch);
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "sync".to_string()
    } else if trimmed.len() == out.len() {
        out
    } else {
        trimmed.to_string()
    }
}

fn shell_escape(value: &str) -> String {
    if value.is_empty() {
        "''".to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

fn remote_path_command(remote: &str) -> String {
    let trimmed = remote.trim();
    if trimmed == "~" || trimmed.starts_with("~") {
        if let Some((prefix, rest)) = trimmed.split_once('/') {
            return format!("{}/{}", prefix, shell_escape(rest));
        }
        return trimmed.to_string();
    }
    shell_escape(trimmed)
}
