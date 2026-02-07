use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;

use crate::model::{AppStateFile, Settings};

pub fn state_file_path() -> Result<PathBuf> {
    let proj = ProjectDirs::from("com", "digitalocean", "doctl-tui")
        .context("Unable to resolve config directory")?;
    let dir = proj.config_dir();
    fs::create_dir_all(dir).context("Failed to create config directory")?;
    Ok(dir.join("state.json"))
}

pub fn load_state() -> Result<AppStateFile> {
    let path = state_file_path()?;
    if !path.exists() {
        return Ok(default_state());
    }
    let data = fs::read_to_string(&path).context("Failed to read state file")?;
    let mut state: AppStateFile =
        serde_json::from_str(&data).context("Failed to parse state file")?;
    if state.settings.default_ssh_user.is_empty() {
        state.settings = default_settings();
    }
    Ok(state)
}

pub fn save_state(state: &AppStateFile) -> Result<()> {
    let path = state_file_path()?;
    let data = serde_json::to_string_pretty(state).context("Failed to serialize state")?;
    fs::write(&path, data).context("Failed to write state file")
}

pub fn default_settings() -> Settings {
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
    Settings {
        default_ssh_user: "root".to_string(),
        default_ssh_key_path: format!("{home}/.ssh/id_rsa"),
        default_ssh_port: 22,
    }
}

pub fn default_state() -> AppStateFile {
    AppStateFile {
        bindings: Vec::new(),
        settings: default_settings(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_uses_home_env() {
        let original = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", "/tmp/doctl-test-home");
        }
        let settings = default_settings();
        assert_eq!(settings.default_ssh_user, "root");
        assert_eq!(settings.default_ssh_port, 22);
        assert_eq!(
            settings.default_ssh_key_path,
            "/tmp/doctl-test-home/.ssh/id_rsa"
        );
        if let Some(value) = original {
            unsafe {
                std::env::set_var("HOME", value);
            }
        }
    }

    #[test]
    fn default_state_is_empty() {
        let state = default_state();
        assert!(state.bindings.is_empty());
        assert_eq!(state.settings.default_ssh_user, "root");
    }
}
