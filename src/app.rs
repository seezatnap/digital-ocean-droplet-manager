use std::collections::HashSet;

use chrono::{DateTime, Utc};
use crossbeam_channel::Sender;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config;
use crate::doctl::CreateDropletArgs;
use crate::input::TextInput;
use crate::model::{AppStateFile, Droplet, Image, Region, Size, Snapshot, SshKey};
use crate::mutagen::{SshConfig, SyncPath, SyncSession};
use crate::ports;
use crate::tasks::{self, Task, TaskResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Bindings,
    Syncs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub level: ToastLevel,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Selection {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct PickerItem {
    pub label: String,
    pub value: String,
    pub meta: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerTarget {
    CreateRegion,
    CreateSize,
    CreateImage,
    CreateSshKeys,
    RestoreSnapshot,
    RestoreRegion,
    RestoreSize,
    RestoreSshKeys,
}

#[derive(Debug, Clone)]
pub struct Picker {
    pub title: String,
    pub items: Vec<PickerItem>,
    pub filtered: Vec<usize>,
    pub selected: usize,
    pub query: TextInput,
    pub multi: bool,
    pub chosen: HashSet<usize>,
    pub target: PickerTarget,
}

#[derive(Debug, Clone)]
pub struct CreateForm {
    pub name: TextInput,
    pub region: Option<Selection>,
    pub size: Option<Selection>,
    pub image: Option<Selection>,
    pub ssh_keys: Vec<Selection>,
    pub tags: TextInput,
    pub focus: usize,
}

#[derive(Debug, Clone)]
pub struct RestoreForm {
    pub name: TextInput,
    pub snapshot: Option<Selection>,
    pub region: Option<Selection>,
    pub size: Option<Selection>,
    pub ssh_keys: Vec<Selection>,
    pub tags: TextInput,
    pub focus: usize,
}

#[derive(Debug, Clone)]
pub struct BindForm {
    pub droplet_id: u64,
    pub droplet_name: String,
    pub public_ip: String,
    pub local_port: TextInput,
    pub remote_port: TextInput,
    pub ssh_user: TextInput,
    pub ssh_key_path: TextInput,
    pub ssh_port: TextInput,
    pub focus: usize,
}

#[derive(Debug, Clone)]
pub struct SyncForm {
    pub droplet_name: String,
    pub public_ip: String,
    pub local_paths: TextInput,
    pub ssh_user: TextInput,
    pub ssh_key_path: TextInput,
    pub ssh_port: TextInput,
    pub focus: usize,
}

#[derive(Debug, Clone)]
pub struct SnapshotForm {
    pub droplet_id: u64,
    pub droplet_name: String,
    pub snapshot_name: TextInput,
}

#[derive(Debug, Clone)]
pub struct Confirm {
    pub title: String,
    pub message: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    SnapshotDelete { droplet_id: u64, snapshot_name: String },
    DeleteDroplet { droplet_id: u64 },
}

#[derive(Debug, Clone)]
pub enum Modal {
    Create(CreateForm),
    Restore(RestoreForm),
    Bind(BindForm),
    Sync(SyncForm),
    Snapshot(SnapshotForm),
    Picker { picker: Picker, parent: Box<Modal> },
    Confirm(Confirm),
}

#[derive(Debug)]
pub struct App {
    pub screen: Screen,
    pub modal: Option<Modal>,
    pub droplets: Vec<Droplet>,
    pub selected: usize,
    pub snapshots: Vec<Snapshot>,
    pub regions: Vec<Region>,
    pub sizes: Vec<Size>,
    pub images: Vec<Image>,
    pub ssh_keys: Vec<SshKey>,
    pub syncs: Vec<SyncSession>,
    pub syncs_context: Option<SshConfig>,
    pub state: AppStateFile,
    pub toast: Option<Toast>,
    pub should_quit: bool,
    pub last_refresh: Option<DateTime<Utc>>,
    pub filter_running: bool,
    pub pending: usize,
    pub task_tx: Sender<TaskResult>,
}

impl App {
    pub fn new(task_tx: Sender<TaskResult>) -> Self {
        let state = config::load_state().unwrap_or_else(|_| config::default_state());
        Self {
            screen: Screen::Home,
            modal: None,
            droplets: Vec::new(),
            selected: 0,
            snapshots: Vec::new(),
            regions: Vec::new(),
            sizes: Vec::new(),
            images: Vec::new(),
            ssh_keys: Vec::new(),
            syncs: Vec::new(),
            syncs_context: None,
            state,
            toast: None,
            should_quit: false,
            last_refresh: None,
            filter_running: false,
            pending: 0,
            task_tx,
        }
    }

    pub fn bootstrap(&mut self) {
        self.spawn(Task::CheckDoctl);
        self.refresh_all();
    }

    pub fn refresh_all(&mut self) {
        self.spawn(Task::RefreshDroplets);
        self.spawn(Task::LoadSnapshots);
        self.spawn(Task::LoadRegions);
        self.spawn(Task::LoadSizes);
        self.spawn(Task::LoadImages);
        self.spawn(Task::LoadSshKeys);
    }

    pub fn spawn(&mut self, task: Task) {
        self.pending += 1;
        tasks::spawn(task, self.task_tx.clone());
    }

    pub fn handle_task_result(&mut self, result: TaskResult) {
        if self.pending > 0 {
            self.pending -= 1;
        }
        match result {
            TaskResult::DoctlCheck(res) => match res {
                Ok(()) => self.push_toast("doctl authenticated", ToastLevel::Success),
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::Droplets(res) => match res {
                Ok(mut droplets) => {
                    droplets.sort_by(|a, b| a.name.cmp(&b.name));
                    self.droplets = droplets;
                    self.selected = 0;
                    self.last_refresh = Some(Utc::now());
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::Snapshots(res) => match res {
                Ok(mut snapshots) => {
                    snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                    self.snapshots = snapshots;
                    let snapshot_items = self.snapshot_picker_items();
                    if let Some(Modal::Picker { picker, .. }) = &mut self.modal {
                        if picker.target == PickerTarget::RestoreSnapshot {
                            picker.items = snapshot_items;
                            picker.refresh_filter();
                        }
                    }
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::Regions(res) => match res {
                Ok(mut regions) => {
                    regions.sort_by(|a, b| a.slug.cmp(&b.slug));
                    self.regions = regions;
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::Sizes(res) => match res {
                Ok(mut sizes) => {
                    sizes.sort_by(|a, b| a.slug.cmp(&b.slug));
                    self.sizes = sizes;
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::Images(res) => match res {
                Ok(mut images) => {
                    images.sort_by(|a, b| a.name.cmp(&b.name));
                    self.images = images;
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::SshKeys(res) => match res {
                Ok(mut keys) => {
                    keys.sort_by(|a, b| a.name.cmp(&b.name));
                    self.ssh_keys = keys;
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::CreateDroplet(res) => match res {
                Ok(droplet) => {
                    self.push_toast("Droplet created", ToastLevel::Success);
                    self.droplets.push(droplet);
                    self.modal = None;
                    self.spawn(Task::RefreshDroplets);
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::RestoreDroplet(res) => match res {
                Ok(droplet) => {
                    self.push_toast("Droplet restored", ToastLevel::Success);
                    self.droplets.push(droplet);
                    self.modal = None;
                    self.spawn(Task::RefreshDroplets);
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::SnapshotDelete(res) => match res {
                Ok(()) => {
                    self.push_toast("Snapshot created and droplet deleted", ToastLevel::Success);
                    self.modal = None;
                    self.spawn(Task::RefreshDroplets);
                    self.spawn(Task::LoadSnapshots);
                    self.spawn(Task::LoadSnapshotsDelayed { delay_ms: 4000 });
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::DeleteDroplet(res) => match res {
                Ok(()) => {
                    self.push_toast("Droplet deleted", ToastLevel::Success);
                    self.modal = None;
                    self.spawn(Task::RefreshDroplets);
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::StartTunnel(res) => match res {
                Ok(binding) => {
                    self.state.bindings.push(binding);
                    let _ = config::save_state(&self.state);
                    self.push_toast("Port bound", ToastLevel::Success);
                    self.modal = None;
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::StopTunnel(res) => match res {
                Ok(port) => {
                    self.state
                        .bindings
                        .retain(|binding| binding.local_port != port);
                    let _ = config::save_state(&self.state);
                    self.push_toast("Port unbound", ToastLevel::Success);
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::CreateSyncs(res) => match res {
                Ok(count) => {
                    self.push_toast(
                        format!("Synced {count} folder{}", if count == 1 { "" } else { "s" }),
                        ToastLevel::Success,
                    );
                    self.modal = None;
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::RestoreSyncs(res) => match res {
                Ok(count) => {
                    self.push_toast(
                        format!(
                            "Restored {count} sync{}",
                            if count == 1 { "" } else { "s" }
                        ),
                        ToastLevel::Success,
                    );
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::Syncs(res) => match res {
                Ok(mut syncs) => {
                    syncs.sort_by(|a, b| a.name.cmp(&b.name));
                    self.syncs = syncs;
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
            TaskResult::DeleteSync(res) => match res {
                Ok(outcome) => {
                    if let Some(err) = outcome.mount_error {
                        self.push_toast(
                            format!(
                                "Sync '{}' terminated, but mount cleanup failed: {err}",
                                outcome.name
                            ),
                            ToastLevel::Warning,
                        );
                    } else if outcome.mount_removed {
                        self.push_toast(
                            format!("Sync '{}' deleted and mount removed", outcome.name),
                            ToastLevel::Success,
                        );
                    } else {
                        self.push_toast(
                            format!("Sync '{}' deleted", outcome.name),
                            ToastLevel::Success,
                        );
                    }
                    self.spawn(Task::LoadSyncs);
                }
                Err(err) => self.push_toast(err.to_string(), ToastLevel::Error),
            },
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if let Some(modal) = self.modal.clone() {
            self.handle_modal_key(modal, key);
            return;
        }

        match self.screen {
            Screen::Home => self.handle_home_key(key),
            Screen::Bindings => self.handle_bindings_key(key),
            Screen::Syncs => self.handle_syncs_key(key),
        }
    }

    fn handle_home_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('g') => self.refresh_all(),
            KeyCode::Char('c') => self.open_create_modal(),
            KeyCode::Char('r') => self.open_restore_modal(),
            KeyCode::Char('s') => self.open_snapshot_modal(),
            KeyCode::Char('d') => self.open_delete_modal(),
            KeyCode::Char('b') => self.open_bind_modal(),
            KeyCode::Char('m') => self.open_sync_modal(),
            KeyCode::Char('u') => self.restore_syncs(),
            KeyCode::Char('y') => self.open_syncs_screen(),
            KeyCode::Char('p') => {
                self.screen = Screen::Bindings;
                self.selected = 0;
            }
            KeyCode::Char('f') => {
                self.filter_running = !self.filter_running;
                self.selected = 0;
            }
            KeyCode::Down => self.move_selection(1),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Enter => self.connect_selected(),
            _ => {}
        }
    }

    fn handle_bindings_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.screen = Screen::Home;
                self.selected = 0;
            }
            KeyCode::Down => self.move_binding_selection(1),
            KeyCode::Up => self.move_binding_selection(-1),
            KeyCode::Char('d') => self.unbind_selected(),
            KeyCode::Char('x') => self.cleanup_stale(),
            _ => {}
        }
    }

    fn handle_syncs_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.screen = Screen::Home;
                self.selected = 0;
            }
            KeyCode::Down => self.move_sync_selection(1),
            KeyCode::Up => self.move_sync_selection(-1),
            KeyCode::Char('d') => self.terminate_selected_sync(),
            KeyCode::Char('g') => self.spawn(Task::LoadSyncs),
            _ => {}
        }
    }

    fn handle_modal_key(&mut self, modal: Modal, key: KeyEvent) {
        match modal {
            Modal::Create(mut form) => {
                if self.handle_create_form_key(&mut form, key) {
                    self.modal = Some(Modal::Create(form));
                }
            }
            Modal::Restore(mut form) => {
                if self.handle_restore_form_key(&mut form, key) {
                    self.modal = Some(Modal::Restore(form));
                }
            }
            Modal::Bind(mut form) => {
                if self.handle_bind_form_key(&mut form, key) {
                    self.modal = Some(Modal::Bind(form));
                }
            }
            Modal::Sync(mut form) => {
                if self.handle_sync_form_key(&mut form, key) {
                    self.modal = Some(Modal::Sync(form));
                }
            }
            Modal::Snapshot(mut form) => {
                if self.handle_snapshot_key(&mut form, key) {
                    self.modal = Some(Modal::Snapshot(form));
                }
            }
            Modal::Picker { mut picker, parent } => {
                let parent_clone = (*parent).clone();
                if self.handle_picker_key(&mut picker, key, parent_clone) {
                    self.modal = Some(Modal::Picker { picker, parent });
                }
            }
            Modal::Confirm(confirm) => {
                self.handle_confirm_key(confirm, key);
            }
        }
    }

    fn handle_create_form_key(&mut self, form: &mut CreateForm, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.modal = None;
                return false;
            }
            KeyCode::Tab | KeyCode::Down => {
                form.focus = (form.focus + 1) % 8;
                return true;
            }
            KeyCode::BackTab | KeyCode::Up => {
                form.focus = (form.focus + 7) % 8;
                return true;
            }
            KeyCode::Enter => {
                match form.focus {
                    0 => form.focus = 1,
                    1 => {
                        self.open_picker(PickerTarget::CreateRegion, Modal::Create(form.clone()), vec![]);
                        return false;
                    }
                    2 => {
                        self.open_picker(PickerTarget::CreateSize, Modal::Create(form.clone()), vec![]);
                        return false;
                    }
                    3 => {
                        self.open_picker(PickerTarget::CreateImage, Modal::Create(form.clone()), vec![]);
                        return false;
                    }
                    4 => {
                        self.open_picker(
                            PickerTarget::CreateSshKeys,
                            Modal::Create(form.clone()),
                            form.ssh_keys.clone(),
                        );
                        return false;
                    }
                    5 => form.focus = 6,
                    6 => {
                        self.submit_create_form(form);
                        return false;
                    }
                    _ => {
                        self.modal = None;
                        return false;
                    }
                }
                return true;
            }
            _ => {}
        }

        if matches!(form.focus, 0 | 5) {
            let input = if form.focus == 0 {
                &mut form.name
            } else {
                &mut form.tags
            };
            handle_text_input(input, key);
        }

        true
    }

    fn handle_restore_form_key(&mut self, form: &mut RestoreForm, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.modal = None;
                return false;
            }
            KeyCode::Tab | KeyCode::Down => {
                form.focus = (form.focus + 1) % 8;
                return true;
            }
            KeyCode::BackTab | KeyCode::Up => {
                form.focus = (form.focus + 7) % 8;
                return true;
            }
            KeyCode::Enter => {
                match form.focus {
                    0 => form.focus = 1,
                    1 => {
                        self.open_picker(
                            PickerTarget::RestoreSnapshot,
                            Modal::Restore(form.clone()),
                            vec![],
                        );
                        return false;
                    }
                    2 => {
                        self.open_picker(
                            PickerTarget::RestoreRegion,
                            Modal::Restore(form.clone()),
                            vec![],
                        );
                        return false;
                    }
                    3 => {
                        self.open_picker(
                            PickerTarget::RestoreSize,
                            Modal::Restore(form.clone()),
                            vec![],
                        );
                        return false;
                    }
                    4 => {
                        self.open_picker(
                            PickerTarget::RestoreSshKeys,
                            Modal::Restore(form.clone()),
                            form.ssh_keys.clone(),
                        );
                        return false;
                    }
                    5 => form.focus = 6,
                    6 => {
                        self.submit_restore_form(form);
                        return false;
                    }
                    _ => {
                        self.modal = None;
                        return false;
                    }
                }
                return true;
            }
            _ => {}
        }

        if matches!(form.focus, 0 | 5) {
            let input = if form.focus == 0 {
                &mut form.name
            } else {
                &mut form.tags
            };
            handle_text_input(input, key);
        }

        true
    }

    fn handle_bind_form_key(&mut self, form: &mut BindForm, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.modal = None;
                return false;
            }
            KeyCode::Tab | KeyCode::Down => {
                form.focus = (form.focus + 1) % 6;
                return true;
            }
            KeyCode::BackTab | KeyCode::Up => {
                form.focus = (form.focus + 5) % 6;
                return true;
            }
            KeyCode::Enter => {
                if form.focus == 5 {
                    self.submit_bind_form(form.clone());
                    return false;
                }
                form.focus = (form.focus + 1) % 6;
                return true;
            }
            _ => {}
        }

        let input = match form.focus {
            0 => &mut form.local_port,
            1 => &mut form.remote_port,
            2 => &mut form.ssh_user,
            3 => &mut form.ssh_key_path,
            4 => &mut form.ssh_port,
            _ => return true,
        };
        handle_text_input(input, key);
        true
    }

    fn handle_sync_form_key(&mut self, form: &mut SyncForm, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.modal = None;
                return false;
            }
            KeyCode::Tab | KeyCode::Down => {
                form.focus = (form.focus + 1) % 6;
                return true;
            }
            KeyCode::BackTab | KeyCode::Up => {
                form.focus = (form.focus + 5) % 6;
                return true;
            }
            KeyCode::Enter => {
                if form.focus == 4 {
                    self.submit_sync_form(form.clone());
                    return false;
                }
                if form.focus == 5 {
                    self.modal = None;
                    return false;
                }
                form.focus = (form.focus + 1) % 6;
                return true;
            }
            _ => {}
        }

        let input = match form.focus {
            0 => &mut form.local_paths,
            1 => &mut form.ssh_user,
            2 => &mut form.ssh_key_path,
            3 => &mut form.ssh_port,
            _ => return true,
        };
        handle_text_input(input, key);
        true
    }

    fn handle_snapshot_key(&mut self, form: &mut SnapshotForm, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.modal = None;
                return false;
            }
            KeyCode::Enter => {
                let name = form.snapshot_name.value.trim().to_string();
                if name.is_empty() {
                    self.push_toast("Snapshot name required", ToastLevel::Warning);
                    return true;
                }
                let confirm = Confirm {
                    title: "Snapshot + Delete".to_string(),
                    message: format!(
                        "Create snapshot '{}' and delete droplet '{}' ?",
                        name, form.droplet_name
                    ),
                    action: ConfirmAction::SnapshotDelete {
                        droplet_id: form.droplet_id,
                        snapshot_name: name,
                    },
                };
                self.modal = Some(Modal::Confirm(confirm));
                return false;
            }
            _ => handle_text_input(&mut form.snapshot_name, key),
        }
        true
    }

    fn handle_picker_key(&mut self, picker: &mut Picker, key: KeyEvent, parent: Modal) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.modal = Some(parent);
                return false;
            }
            KeyCode::Up => {
                if picker.selected > 0 {
                    picker.selected -= 1;
                }
            }
            KeyCode::Down => {
                if picker.selected + 1 < picker.filtered.len() {
                    picker.selected += 1;
                }
            }
            KeyCode::Char(' ') if picker.multi => {
                if let Some(&idx) = picker.filtered.get(picker.selected) {
                    if picker.chosen.contains(&idx) {
                        picker.chosen.remove(&idx);
                    } else {
                        picker.chosen.insert(idx);
                    }
                }
            }
            KeyCode::Enter => {
                self.apply_picker_selection(picker.clone(), parent);
                return false;
            }
            KeyCode::Backspace => {
                picker.query.backspace();
                picker.refresh_filter();
            }
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    picker.query.insert(ch);
                    picker.refresh_filter();
                }
            }
            _ => {}
        }
        true
    }

    fn handle_confirm_key(&mut self, confirm: Confirm, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => match confirm.action {
                ConfirmAction::SnapshotDelete {
                    droplet_id,
                    snapshot_name,
                } => {
                    self.spawn(Task::SnapshotDelete {
                        droplet_id,
                        snapshot_name,
                    });
                    self.modal = None;
                }
                ConfirmAction::DeleteDroplet { droplet_id } => {
                    self.spawn(Task::DeleteDroplet { droplet_id });
                    self.modal = None;
                }
            },
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.modal = None;
            }
            _ => {}
        }
    }

    fn open_create_modal(&mut self) {
        let form = CreateForm {
            name: TextInput::new(""),
            region: None,
            size: None,
            image: None,
            ssh_keys: Vec::new(),
            tags: TextInput::new(""),
            focus: 0,
        };
        self.modal = Some(Modal::Create(form));
    }

    fn open_restore_modal(&mut self) {
        self.spawn(Task::LoadSnapshots);
        let form = RestoreForm {
            name: TextInput::new(""),
            snapshot: None,
            region: None,
            size: None,
            ssh_keys: Vec::new(),
            tags: TextInput::new(""),
            focus: 0,
        };
        self.modal = Some(Modal::Restore(form));
    }

    fn open_bind_modal(&mut self) {
        let droplet = match self.selected_droplet() {
            Some(droplet) => droplet.clone(),
            None => {
                self.push_toast("No droplet selected", ToastLevel::Warning);
                return;
            }
        };
        if !droplet.is_running() {
            self.push_toast("Droplet must be running", ToastLevel::Warning);
            return;
        }
        let public_ip = match droplet.public_ipv4.clone() {
            Some(ip) => ip,
            None => {
                self.push_toast("Droplet has no public IP", ToastLevel::Warning);
                return;
            }
        };
        let settings = &self.state.settings;
        let form = BindForm {
            droplet_id: droplet.id,
            droplet_name: droplet.name,
            public_ip,
            local_port: TextInput::new(""),
            remote_port: TextInput::new(""),
            ssh_user: TextInput::new(settings.default_ssh_user.clone()),
            ssh_key_path: TextInput::new(settings.default_ssh_key_path.clone()),
            ssh_port: TextInput::new(settings.default_ssh_port.to_string()),
            focus: 0,
        };
        self.modal = Some(Modal::Bind(form));
    }

    fn open_sync_modal(&mut self) {
        let droplet = match self.selected_droplet() {
            Some(droplet) => droplet.clone(),
            None => {
                self.push_toast("No droplet selected", ToastLevel::Warning);
                return;
            }
        };
        if !droplet.is_running() {
            self.push_toast("Droplet must be running", ToastLevel::Warning);
            return;
        }
        let public_ip = match droplet.public_ipv4.clone() {
            Some(ip) => ip,
            None => {
                self.push_toast("Droplet has no public IP", ToastLevel::Warning);
                return;
            }
        };
        let settings = &self.state.settings;
        let form = SyncForm {
            droplet_name: droplet.name,
            public_ip,
            local_paths: TextInput::new(""),
            ssh_user: TextInput::new(settings.default_ssh_user.clone()),
            ssh_key_path: TextInput::new(settings.default_ssh_key_path.clone()),
            ssh_port: TextInput::new(settings.default_ssh_port.to_string()),
            focus: 0,
        };
        self.modal = Some(Modal::Sync(form));
    }

    fn open_snapshot_modal(&mut self) {
        let droplet = match self.selected_droplet() {
            Some(droplet) => droplet.clone(),
            None => {
                self.push_toast("No droplet selected", ToastLevel::Warning);
                return;
            }
        };
        if !droplet.is_running() {
            self.push_toast("Droplet must be running", ToastLevel::Warning);
            return;
        }
        let snapshot_name = format!(
            "{}-{}",
            sanitize_name(&droplet.name),
            Utc::now().format("%Y%m%d-%H%M%S")
        );
        let form = SnapshotForm {
            droplet_id: droplet.id,
            droplet_name: droplet.name,
            snapshot_name: TextInput::new(snapshot_name),
        };
        self.modal = Some(Modal::Snapshot(form));
    }

    fn open_delete_modal(&mut self) {
        let droplet = match self.selected_droplet() {
            Some(droplet) => droplet.clone(),
            None => {
                self.push_toast("No droplet selected", ToastLevel::Warning);
                return;
            }
        };
        let confirm = Confirm {
            title: "Delete Droplet".to_string(),
            message: format!(
                "Delete droplet '{}' (#{}). This is irreversible.",
                droplet.name, droplet.id
            ),
            action: ConfirmAction::DeleteDroplet {
                droplet_id: droplet.id,
            },
        };
        self.modal = Some(Modal::Confirm(confirm));
    }

    fn open_picker(&mut self, target: PickerTarget, parent: Modal, preselected: Vec<Selection>) {
        let (title, items, multi) = match target {
            PickerTarget::CreateRegion | PickerTarget::RestoreRegion => {
                if self.regions.is_empty() {
                    self.push_toast("No regions loaded (press g to refresh)", ToastLevel::Warning);
                    return;
                }
                let mut available: Vec<&Region> =
                    self.regions.iter().filter(|r| r.available).collect();
                if available.is_empty() {
                    available = self.regions.iter().collect();
                }
                let items = available
                    .into_iter()
                    .map(|region| PickerItem {
                        label: if region.available {
                            format!("{} ({})", region.slug, region.name)
                        } else {
                            format!("{} ({}, unavailable)", region.slug, region.name)
                        },
                        value: region.slug.clone(),
                        meta: Some(region.name.clone()),
                    })
                    .collect();
                ("Select Region".to_string(), items, false)
            }
            PickerTarget::CreateSize | PickerTarget::RestoreSize => {
                let items = self
                    .sizes
                    .iter()
                    .map(|size| PickerItem {
                        label: format!(
                            "{} ({}MB, {} vCPU, {}GB)",
                            size.slug, size.memory_mb, size.vcpus, size.disk_gb
                        ),
                        value: size.slug.clone(),
                        meta: Some(format!("${:.2}/mo", size.price_monthly)),
                    })
                    .collect();
                ("Select Size".to_string(), items, false)
            }
            PickerTarget::CreateImage => {
                let items = self
                    .images
                    .iter()
                    .map(|image| PickerItem {
                        label: format!(
                            "{}{}",
                            image.name,
                            image
                                .slug
                                .as_ref()
                                .map(|slug| format!(" ({slug})"))
                                .unwrap_or_default()
                        ),
                        value: image
                            .slug
                            .clone()
                            .unwrap_or_else(|| image.id.to_string()),
                        meta: image.distribution.clone(),
                    })
                    .collect();
                ("Select Image".to_string(), items, false)
            }
            PickerTarget::CreateSshKeys | PickerTarget::RestoreSshKeys => {
                let items = self
                    .ssh_keys
                    .iter()
                    .map(|key| PickerItem {
                        label: format!("{} ({})", key.name, key.fingerprint),
                        value: key.id.to_string(),
                        meta: None,
                    })
                    .collect();
                ("Select SSH Keys".to_string(), items, true)
            }
            PickerTarget::RestoreSnapshot => {
                if self.snapshots.is_empty() {
                    self.push_toast("No snapshots loaded yet (refreshing)", ToastLevel::Warning);
                    self.spawn(Task::LoadSnapshots);
                    return;
                }
                let items = self.snapshot_picker_items();
                ("Select Snapshot".to_string(), items, false)
            }
        };

        let mut picker = Picker::new(title, items, target, multi);
        if picker.multi {
            for (idx, item) in picker.items.iter().enumerate() {
                if preselected.iter().any(|sel| sel.value == item.value) {
                    picker.chosen.insert(idx);
                }
            }
        }

        self.modal = Some(Modal::Picker {
            picker,
            parent: Box::new(parent),
        });
    }

    fn apply_picker_selection(&mut self, picker: Picker, mut parent: Modal) {
        let selected_items: Vec<PickerItem> = if picker.multi {
            picker
                .chosen
                .iter()
                .filter_map(|idx| picker.items.get(*idx).cloned())
                .collect()
        } else {
            picker
                .filtered
                .get(picker.selected)
                .and_then(|idx| picker.items.get(*idx))
                .cloned()
                .map(|item| vec![item])
                .unwrap_or_default()
        };

        let to_selection = |item: PickerItem| Selection {
            label: item.label,
            value: item.value,
        };

        match picker.target {
            PickerTarget::CreateRegion => {
                if let Modal::Create(form) = &mut parent {
                    form.region = selected_items.first().cloned().map(to_selection);
                }
            }
            PickerTarget::CreateSize => {
                if let Modal::Create(form) = &mut parent {
                    form.size = selected_items.first().cloned().map(to_selection);
                }
            }
            PickerTarget::CreateImage => {
                if let Modal::Create(form) = &mut parent {
                    form.image = selected_items.first().cloned().map(to_selection);
                }
            }
            PickerTarget::CreateSshKeys => {
                if let Modal::Create(form) = &mut parent {
                    form.ssh_keys = selected_items.into_iter().map(to_selection).collect();
                }
            }
            PickerTarget::RestoreSnapshot => {
                if let Modal::Restore(form) = &mut parent {
                    form.snapshot = selected_items.first().cloned().map(to_selection);
                }
            }
            PickerTarget::RestoreRegion => {
                if let Modal::Restore(form) = &mut parent {
                    form.region = selected_items.first().cloned().map(to_selection);
                }
            }
            PickerTarget::RestoreSize => {
                if let Modal::Restore(form) = &mut parent {
                    form.size = selected_items.first().cloned().map(to_selection);
                }
            }
            PickerTarget::RestoreSshKeys => {
                if let Modal::Restore(form) = &mut parent {
                    form.ssh_keys = selected_items.into_iter().map(to_selection).collect();
                }
            }
        }

        self.modal = Some(parent);
    }

    fn submit_create_form(&mut self, form: &CreateForm) {
        let name = form.name.value.trim();
        if name.is_empty() {
            self.push_toast("Name is required", ToastLevel::Warning);
            return;
        }
        let size = match &form.size {
            Some(size) => size.value.clone(),
            None => {
                self.push_toast("Size is required", ToastLevel::Warning);
                return;
            }
        };
        let image = match &form.image {
            Some(image) => image.value.clone(),
            None => {
                self.push_toast("Image is required", ToastLevel::Warning);
                return;
            }
        };

        let args = CreateDropletArgs {
            name: name.to_string(),
            region: form.region.as_ref().map(|region| region.value.clone()),
            size,
            image,
            ssh_keys: form.ssh_keys.iter().map(|k| k.value.clone()).collect(),
            tags: split_csv(&form.tags.value),
        };

        self.spawn(Task::CreateDroplet(args));
    }

    fn submit_restore_form(&mut self, form: &RestoreForm) {
        let name = form.name.value.trim();
        if name.is_empty() {
            self.push_toast("Name is required", ToastLevel::Warning);
            return;
        }
        let snapshot = match &form.snapshot {
            Some(snapshot) => snapshot.value.clone(),
            None => {
                self.push_toast("Snapshot is required", ToastLevel::Warning);
                return;
            }
        };
        let size = match &form.size {
            Some(size) => size.value.clone(),
            None => {
                self.push_toast("Size is required", ToastLevel::Warning);
                return;
            }
        };
        let args = CreateDropletArgs {
            name: name.to_string(),
            region: form.region.as_ref().map(|region| region.value.clone()),
            size,
            image: snapshot,
            ssh_keys: form.ssh_keys.iter().map(|k| k.value.clone()).collect(),
            tags: split_csv(&form.tags.value),
        };

        self.spawn(Task::RestoreDroplet(args));
    }

    fn submit_bind_form(&mut self, form: BindForm) {
        let local_port = match form.local_port.value.trim().parse::<u16>() {
            Ok(port) => port,
            Err(_) => {
                self.push_toast("Invalid local port", ToastLevel::Warning);
                return;
            }
        };
        let remote_port = match form.remote_port.value.trim().parse::<u16>() {
            Ok(port) => port,
            Err(_) => {
                self.push_toast("Invalid remote port", ToastLevel::Warning);
                return;
            }
        };
        let ssh_port = match form.ssh_port.value.trim().parse::<u16>() {
            Ok(port) => port,
            Err(_) => {
                self.push_toast("Invalid SSH port", ToastLevel::Warning);
                return;
            }
        };

        if ports::port_in_registry(&self.state, local_port).is_some() {
            self.push_toast("Local port already bound", ToastLevel::Warning);
            return;
        }

        if !ports::is_port_available(local_port) {
            self.push_toast("Local port is in use", ToastLevel::Warning);
            return;
        }

        let binding = ports::new_binding(
            form.droplet_id,
            form.droplet_name,
            form.public_ip,
            local_port,
            remote_port,
            form.ssh_user.value.trim().to_string(),
            form.ssh_key_path.value.trim().to_string(),
            ssh_port,
        );

        self.spawn(Task::StartTunnel(binding));
    }

    fn submit_sync_form(&mut self, form: SyncForm) {
        let paths = match parse_sync_paths(&form.local_paths.value) {
            Ok(paths) => paths,
            Err(err) => {
                self.push_toast(err.to_string(), ToastLevel::Warning);
                return;
            }
        };
        let ssh_port = match form.ssh_port.value.trim().parse::<u16>() {
            Ok(port) => port,
            Err(_) => {
                self.push_toast("Invalid SSH port", ToastLevel::Warning);
                return;
            }
        };

        let ssh = SshConfig {
            user: form.ssh_user.value.trim().to_string(),
            host: form.public_ip.clone(),
            port: ssh_port,
            key_path: form.ssh_key_path.value.trim().to_string(),
        };

        self.spawn(Task::CreateSyncs {
            ssh,
            droplet_name: form.droplet_name.clone(),
            paths,
        });
    }

    fn restore_syncs(&mut self) {
        match self.selected_ssh_config() {
            Ok(ssh) => self.spawn(Task::RestoreSyncs { ssh }),
            Err(err) => self.push_toast(err.to_string(), ToastLevel::Warning),
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let indices = self.visible_indices();
        if indices.is_empty() {
            self.selected = 0;
            return;
        }
        let max = indices.len() as i32 - 1;
        let mut next = self.selected as i32 + delta;
        if next < 0 {
            next = 0;
        } else if next > max {
            next = max;
        }
        self.selected = next as usize;
    }

    fn move_binding_selection(&mut self, delta: i32) {
        if self.state.bindings.is_empty() {
            self.selected = 0;
            return;
        }
        let max = self.state.bindings.len() as i32 - 1;
        let mut next = self.selected as i32 + delta;
        if next < 0 {
            next = 0;
        } else if next > max {
            next = max;
        }
        self.selected = next as usize;
    }

    fn move_sync_selection(&mut self, delta: i32) {
        if self.syncs.is_empty() {
            self.selected = 0;
            return;
        }
        let max = self.syncs.len() as i32 - 1;
        let mut next = self.selected as i32 + delta;
        if next < 0 {
            next = 0;
        } else if next > max {
            next = max;
        }
        self.selected = next as usize;
    }

    fn connect_selected(&mut self) {
        let droplet = match self.selected_droplet() {
            Some(droplet) => droplet,
            None => {
                self.push_toast("No droplet selected", ToastLevel::Warning);
                return;
            }
        };
        if !droplet.is_running() {
            self.push_toast("Droplet must be running", ToastLevel::Warning);
            return;
        }
        let droplet_id = droplet.id.to_string();
        if let Err(err) = crate::ui::run_interactive(&["compute", "ssh", &droplet_id]) {
            self.push_toast(err.to_string(), ToastLevel::Error);
        }
    }

    fn cleanup_stale(&mut self) {
        let before = self.state.bindings.len();
        self.state
            .bindings
            .retain(|binding| binding.tunnel_pid.map(ports::is_pid_running).unwrap_or(false));
        let removed = before.saturating_sub(self.state.bindings.len());
        if removed > 0 {
            let _ = config::save_state(&self.state);
            self.push_toast(format!("Removed {removed} stale bindings"), ToastLevel::Info);
        } else {
            self.push_toast("No stale bindings found", ToastLevel::Info);
        }
    }

    fn unbind_selected(&mut self) {
        if self.state.bindings.is_empty() {
            return;
        }
        if let Some(binding) = self.state.bindings.get(self.selected).cloned() {
            if let Some(pid) = binding.tunnel_pid {
                self.spawn(Task::StopTunnel {
                    port: binding.local_port,
                    pid,
                });
            } else {
                self.state
                    .bindings
                    .retain(|item| item.local_port != binding.local_port);
                let _ = config::save_state(&self.state);
            }
        }
    }

    fn open_syncs_screen(&mut self) {
        self.screen = Screen::Syncs;
        self.selected = 0;
        self.syncs_context = self.selected_ssh_config().ok();
        self.spawn(Task::LoadSyncs);
    }

    fn terminate_selected_sync(&mut self) {
        if self.syncs.is_empty() {
            return;
        }
        if let Some(sync) = self.syncs.get(self.selected).cloned() {
            let ssh = self.syncs_context.clone();
            self.spawn(Task::DeleteSync { name: sync.name, ssh });
        }
    }

    fn selected_ssh_config(&self) -> anyhow::Result<SshConfig> {
        let droplet = self
            .selected_droplet()
            .ok_or_else(|| anyhow::anyhow!("No droplet selected"))?;
        if !droplet.is_running() {
            return Err(anyhow::anyhow!("Droplet must be running"));
        }
        let public_ip = droplet
            .public_ipv4
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Droplet has no public IP"))?;
        let settings = &self.state.settings;
        Ok(SshConfig {
            user: settings.default_ssh_user.clone(),
            host: public_ip,
            port: settings.default_ssh_port,
            key_path: settings.default_ssh_key_path.clone(),
        })
    }

    pub(crate) fn selected_droplet(&self) -> Option<&Droplet> {
        let indices = self.visible_indices();
        indices
            .get(self.selected)
            .and_then(|idx| self.droplets.get(*idx))
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        self.droplets
            .iter()
            .enumerate()
            .filter_map(|(idx, droplet)| {
                if self.filter_running && !droplet.is_running() {
                    None
                } else {
                    Some(idx)
                }
            })
            .collect()
    }

    pub fn push_toast(&mut self, message: impl Into<String>, level: ToastLevel) {
        self.toast = Some(Toast {
            message: message.into(),
            level,
            created_at: Utc::now(),
        });
    }

    pub fn shutdown(&mut self) {
        for binding in &self.state.bindings {
            if let Some(pid) = binding.tunnel_pid {
                let _ = ports::stop_tunnel(pid);
            }
        }
        let _ = config::save_state(&self.state);
    }

    fn snapshot_picker_items(&self) -> Vec<PickerItem> {
        self.snapshots
            .iter()
            .map(|snap| PickerItem {
                label: format!("{} ({})", snap.name, snap.created_at),
                value: snap.id.to_string(),
                meta: None,
            })
            .collect()
    }
}

impl Picker {
    pub fn new(title: String, items: Vec<PickerItem>, target: PickerTarget, multi: bool) -> Self {
        let mut picker = Self {
            title,
            items,
            filtered: Vec::new(),
            selected: 0,
            query: TextInput::new(""),
            multi,
            chosen: HashSet::new(),
            target,
        };
        picker.refresh_filter();
        picker
    }

    pub fn refresh_filter(&mut self) {
        let query = self.query.value.to_lowercase();
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                if query.is_empty()
                    || item.label.to_lowercase().contains(&query)
                    || item
                        .meta
                        .as_ref()
                        .map(|meta| meta.to_lowercase().contains(&query))
                        .unwrap_or(false)
                {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
    }
}

fn handle_text_input(input: &mut TextInput, key: KeyEvent) {
    match key.code {
        KeyCode::Char(ch) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                return;
            }
            input.insert(ch);
        }
        KeyCode::Backspace => input.backspace(),
        KeyCode::Delete => input.delete(),
        KeyCode::Left => input.move_left(),
        KeyCode::Right => input.move_right(),
        KeyCode::Home => input.cursor = 0,
        KeyCode::End => input.cursor = input.value.len(),
        _ => {}
    }
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .collect()
}

fn parse_sync_paths(value: &str) -> anyhow::Result<Vec<SyncPath>> {
    let items = split_csv(value);
    if items.is_empty() {
        return Err(anyhow::anyhow!("Provide at least one local path"));
    }
    let mut paths = Vec::new();
    for item in items {
        let parts: Vec<&str> = item.splitn(2, "->").collect();
        let local = parts
            .get(0)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Local path cannot be empty"))?;
        let remote = if parts.len() == 2 {
            parts[1].trim()
        } else {
            local
        };
        if remote.is_empty() {
            return Err(anyhow::anyhow!("Remote path cannot be empty"));
        }
        paths.push(SyncPath {
            local: local.to_string(),
            remote: remote.to_string(),
        });
    }
    Ok(paths)
}

fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = false;
    for ch in name.trim().chars() {
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
        "droplet".to_string()
    } else if trimmed.len() == out.len() {
        out
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::split_csv;

    #[test]
    fn split_csv_trims_and_filters() {
        let values = split_csv(" alpha, beta , ,gamma,, ");
        assert_eq!(values, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn split_csv_empty_returns_empty_vec() {
        let values = split_csv("   ");
        assert!(values.is_empty());
    }
}
