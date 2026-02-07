use anyhow::{Context, anyhow};
use chrono::Utc;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{ExecutableCommand, execute};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use std::io;

use crate::app::{
    App, BindForm, CreateForm, DeleteRsyncBindForm, Modal, Notice, Picker, RemoteBrowserForm,
    RestoreForm, RsyncBindActionsForm, RsyncBindForm, Screen, SnapshotForm, SyncForm, ToastLevel,
};
use crate::input::TextInput;
use crate::ports;

pub struct Theme {
    pub bg: Color,
    pub muted: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub border: Color,
}

impl Theme {
    pub fn default() -> Self {
        Self {
            bg: Color::Rgb(15, 17, 20),
            muted: Color::Rgb(130, 130, 130),
            accent: Color::Rgb(0, 180, 170),
            success: Color::Rgb(0, 200, 120),
            warning: Color::Rgb(240, 180, 80),
            error: Color::Rgb(235, 80, 80),
            border: Color::Rgb(60, 60, 70),
        }
    }
}

pub fn setup_terminal() -> anyhow::Result<Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>>
{
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore_terminal(
    mut terminal: Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
) -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

pub fn run_interactive(args: &[&str]) -> anyhow::Result<()> {
    disable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(LeaveAlternateScreen)?;
    stdout.execute(DisableMouseCapture)?;
    stdout.execute(crossterm::cursor::Show)?;

    let status = std::process::Command::new("doctl").args(args).status()?;

    stdout.execute(EnterAlternateScreen)?;
    stdout.execute(EnableMouseCapture)?;
    stdout.execute(crossterm::cursor::Hide)?;
    enable_raw_mode()?;

    if !status.success() {
        return Err(anyhow!("doctl command failed"));
    }
    Ok(())
}

pub fn run_external(program: &str, args: &[String]) -> anyhow::Result<()> {
    disable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(LeaveAlternateScreen)?;
    stdout.execute(DisableMouseCapture)?;
    stdout.execute(crossterm::cursor::Show)?;

    let status = std::process::Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("Failed to execute {program}"))?;

    stdout.execute(EnterAlternateScreen)?;
    stdout.execute(EnableMouseCapture)?;
    stdout.execute(crossterm::cursor::Hide)?;
    enable_raw_mode()?;

    if !status.success() {
        return Err(anyhow!("{program} command failed"));
    }
    Ok(())
}

pub fn draw(frame: &mut Frame, app: &App) {
    let theme = Theme::default();
    let area = frame.size();
    frame.render_widget(Block::default().style(Style::default().bg(theme.bg)), area);

    match app.screen {
        Screen::Home => draw_home(frame, app, &theme),
        Screen::Bindings => draw_bindings(frame, app, &theme),
        Screen::Syncs => draw_syncs(frame, app, &theme),
        Screen::RsyncBinds => draw_rsync_binds(frame, app, &theme),
    }

    if let Some(modal) = &app.modal {
        draw_modal(frame, app, modal, &theme);
    }

    draw_toast(frame, app, &theme);
    draw_loading_overlay(frame, app, &theme);
}

fn draw_home(frame: &mut Frame, app: &App, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(frame.size());

    draw_header(frame, app, theme, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(chunks[1]);

    draw_droplet_list(frame, app, theme, body[0]);
    draw_droplet_details(frame, app, theme, body[1]);

    draw_footer(frame, app, theme, chunks[2]);
}

fn draw_bindings(frame: &mut Frame, app: &App, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(frame.size());

    let header = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("Port Bindings")
        .title_alignment(Alignment::Left);
    let title = Paragraph::new(Line::from(vec![
        Span::styled("Active Port Bindings", Style::default().fg(theme.accent)),
        Span::raw("  (press q to return)"),
    ]))
    .block(header);
    frame.render_widget(title, chunks[0]);

    let items: Vec<ListItem> = app
        .state
        .bindings
        .iter()
        .map(|binding| {
            let active = binding
                .tunnel_pid
                .map(ports::is_pid_running)
                .unwrap_or(false);
            let status = if active { "*" } else { "o" };
            let status_style = if active {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.muted)
            };
            let line = Line::from(vec![
                Span::styled(status, status_style),
                Span::raw(format!(
                    "  {}:{} -> {}:{}  ",
                    binding.droplet_name, binding.remote_port, "localhost", binding.local_port
                )),
                Span::styled(
                    format!("{}", binding.public_ip),
                    Style::default().fg(theme.muted),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .title("Port Bindings"),
        )
        .highlight_style(
            Style::default()
                .bg(theme.accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = binding_state_list(app);
    frame.render_stateful_widget(list, chunks[1], &mut state);

    let help = Paragraph::new(Line::from(vec![
        Span::styled("d", Style::default().fg(theme.accent)),
        Span::raw(" unbind  "),
        Span::styled("x", Style::default().fg(theme.accent)),
        Span::raw(" cleanup stale  "),
        Span::styled("q", Style::default().fg(theme.accent)),
        Span::raw(" back"),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border)),
    );
    frame.render_widget(help, chunks[2]);
}

fn draw_syncs(frame: &mut Frame, app: &App, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(frame.size());

    let header = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("Syncs")
        .title_alignment(Alignment::Left);
    let title = Paragraph::new(Line::from(vec![
        Span::styled("Mutagen Sync Sessions", Style::default().fg(theme.accent)),
        Span::raw("  (press q to return)"),
    ]))
    .block(header);
    frame.render_widget(title, chunks[0]);

    let items: Vec<ListItem> = app
        .syncs
        .iter()
        .map(|sync| {
            let status = sync.status.as_deref().unwrap_or("unknown");
            let status_style = if status.eq_ignore_ascii_case("watching")
                || status.eq_ignore_ascii_case("syncing")
                || status.eq_ignore_ascii_case("monitoring")
            {
                Style::default().fg(theme.success)
            } else if status.eq_ignore_ascii_case("paused")
                || status.eq_ignore_ascii_case("stopped")
            {
                Style::default().fg(theme.warning)
            } else {
                Style::default().fg(theme.muted)
            };
            let line = Line::from(vec![
                Span::styled("• ", Style::default().fg(theme.muted)),
                Span::raw(&sync.name),
                Span::raw("  "),
                Span::styled(format!("{status}"), status_style),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .title("Sessions"),
        )
        .highlight_style(
            Style::default()
                .bg(theme.accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = ratatui::widgets::ListState::default();
    if !app.syncs.is_empty() {
        state.select(Some(app.selected.min(app.syncs.len() - 1)));
    }
    frame.render_stateful_widget(list, chunks[1], &mut state);

    let help = Paragraph::new(Line::from(vec![
        Span::styled("d", Style::default().fg(theme.accent)),
        Span::raw(" delete  "),
        Span::styled("g", Style::default().fg(theme.accent)),
        Span::raw(" refresh  "),
        Span::styled("q", Style::default().fg(theme.accent)),
        Span::raw(" back"),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border)),
    );
    frame.render_widget(help, chunks[2]);
}

fn draw_rsync_binds(frame: &mut Frame, app: &App, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(frame.size());

    let header = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("RSYNC Binds")
        .title_alignment(Alignment::Left);
    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "Remote <-> Local Bindings",
            Style::default().fg(theme.accent),
        ),
        Span::raw("  (press q to return)"),
    ]))
    .block(header);
    frame.render_widget(title, chunks[0]);

    let items: Vec<ListItem> = if app.state.rsync_binds.is_empty() {
        vec![ListItem::new(Line::from(vec![Span::styled(
            "<no rsync binds>",
            Style::default().fg(theme.muted),
        )]))]
    } else {
        app.state
            .rsync_binds
            .iter()
            .map(|bind| {
                let line = Line::from(vec![
                    Span::styled("• ", Style::default().fg(theme.muted)),
                    Span::raw(format!("{}  ", bind.droplet_name)),
                    Span::styled(
                        format!("{}@{}:{} ", bind.ssh_user, bind.host, bind.remote_path),
                        Style::default().fg(theme.accent),
                    ),
                    Span::raw(" -> "),
                    Span::styled(&bind.local_path, Style::default().fg(theme.muted)),
                ]);
                ListItem::new(line)
            })
            .collect()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .title("Registry"),
        )
        .highlight_style(
            Style::default()
                .bg(theme.accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = rsync_bind_state_list(app);
    frame.render_stateful_widget(list, chunks[1], &mut state);

    let help = Paragraph::new(Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent)),
        Span::raw(" open bind actions  "),
        Span::styled("?", Style::default().fg(theme.accent)),
        Span::raw(" shortcuts  "),
        Span::styled("q", Style::default().fg(theme.accent)),
        Span::raw(" back"),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border)),
    );
    frame.render_widget(help, chunks[2]);
}

fn draw_header(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let title = Line::from(vec![
        Span::styled(
            "DOCTL",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Droplet Manager"),
    ]);

    let mut right = Vec::new();
    if let Some(last) = app.last_refresh {
        right.push(Span::styled(
            format!("Last refresh {}", last.format("%H:%M:%S")),
            Style::default().fg(theme.muted),
        ));
    }
    if app.pending > 0 {
        right.push(Span::styled("  *", Style::default().fg(theme.accent)));
    }
    if app.filter_running {
        right.push(Span::styled(
            "  [running]",
            Style::default().fg(theme.warning),
        ));
    }

    let header = Paragraph::new(title)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .title_alignment(Alignment::Left),
        )
        .alignment(Alignment::Left);
    frame.render_widget(header, area);

    if !right.is_empty() {
        let right_line = Paragraph::new(Line::from(right))
            .alignment(Alignment::Right)
            .block(Block::default());
        frame.render_widget(right_line, area);
    }
}

fn draw_droplet_list(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let indices = app.visible_indices();
    let items: Vec<ListItem> = indices
        .iter()
        .filter_map(|idx| app.droplets.get(*idx))
        .map(|droplet| {
            let status = if droplet.is_running() { "*" } else { "o" };
            let status_style = if droplet.is_running() {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.muted)
            };
            let line = Line::from(vec![
                Span::styled(status, status_style),
                Span::raw(format!("  {}", droplet.name)),
                Span::styled(
                    format!("  #{}", droplet.id),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(
                    format!("  {}", droplet.region),
                    Style::default().fg(theme.muted),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .title("Droplets"),
        )
        .highlight_style(
            Style::default()
                .bg(theme.accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = app_state_list(app);
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_droplet_details(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let droplet = app.selected_droplet();
    let mut lines = Vec::new();

    if let Some(droplet) = droplet {
        lines.push(Line::from(vec![
            Span::styled("Name: ", Style::default().fg(theme.muted)),
            Span::raw(&droplet.name),
        ]));
        lines.push(Line::from(vec![
            Span::styled("ID: ", Style::default().fg(theme.muted)),
            Span::raw(droplet.id.to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().fg(theme.muted)),
            Span::styled(
                &droplet.status,
                if droplet.is_running() {
                    Style::default().fg(theme.success)
                } else {
                    Style::default().fg(theme.warning)
                },
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Region: ", Style::default().fg(theme.muted)),
            Span::raw(&droplet.region),
        ]));
        if let Some(size) = &droplet.size {
            lines.push(Line::from(vec![
                Span::styled("Size: ", Style::default().fg(theme.muted)),
                Span::raw(size),
            ]));
        }
        if let Some(ip) = &droplet.public_ipv4 {
            lines.push(Line::from(vec![
                Span::styled("Public IP: ", Style::default().fg(theme.muted)),
                Span::raw(ip),
            ]));
        }
        if let Some(ip) = &droplet.private_ipv4 {
            lines.push(Line::from(vec![
                Span::styled("Private IP: ", Style::default().fg(theme.muted)),
                Span::raw(ip),
            ]));
        }
        if !droplet.tags.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Tags: ", Style::default().fg(theme.muted)),
                Span::raw(droplet.tags.join(", ")),
            ]));
        }
        if let Some(created_at) = &droplet.created_at {
            lines.push(Line::from(vec![
                Span::styled("Created: ", Style::default().fg(theme.muted)),
                Span::raw(created_at),
            ]));
        }
    } else {
        lines.push(Line::from("No droplet selected"));
    }

    let actions = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Enter", Style::default().fg(theme.accent)),
            Span::raw(" connect"),
        ]),
        Line::from(vec![
            Span::styled("c", Style::default().fg(theme.accent)),
            Span::raw(" create"),
        ]),
        Line::from(vec![
            Span::styled("s", Style::default().fg(theme.accent)),
            Span::raw(" snapshot+delete"),
        ]),
        Line::from(vec![
            Span::styled("d", Style::default().fg(theme.accent)),
            Span::raw(" delete"),
        ]),
        Line::from(vec![
            Span::styled("r", Style::default().fg(theme.accent)),
            Span::raw(" restore"),
        ]),
        Line::from(vec![
            Span::styled("b", Style::default().fg(theme.accent)),
            Span::raw(" bind port"),
        ]),
        Line::from(vec![
            Span::styled("p", Style::default().fg(theme.accent)),
            Span::raw(" port bindings"),
        ]),
        Line::from(vec![
            Span::styled("m", Style::default().fg(theme.accent)),
            Span::raw(" mutagen config"),
        ]),
        Line::from(vec![
            Span::styled("o", Style::default().fg(theme.accent)),
            Span::raw(" open remote folder"),
        ]),
        Line::from(vec![
            Span::styled("u", Style::default().fg(theme.accent)),
            Span::raw(" rsync binds"),
        ]),
    ];

    let content = lines
        .into_iter()
        .chain(actions.into_iter())
        .collect::<Vec<_>>();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("Details");
    frame.render_widget(
        Paragraph::new(content)
            .block(block)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_footer(frame: &mut Frame, _app: &App, theme: &Theme, area: Rect) {
    let help = Line::from(vec![
        Span::styled("g", Style::default().fg(theme.accent)),
        Span::raw(" refresh  "),
        Span::styled("m", Style::default().fg(theme.accent)),
        Span::raw(" mutagen  "),
        Span::styled("o", Style::default().fg(theme.accent)),
        Span::raw(" open folder  "),
        Span::styled("u", Style::default().fg(theme.accent)),
        Span::raw(" rsync binds  "),
        Span::styled("d", Style::default().fg(theme.accent)),
        Span::raw(" delete  "),
        Span::styled("f", Style::default().fg(theme.accent)),
        Span::raw(" filter running  "),
        Span::styled("p", Style::default().fg(theme.accent)),
        Span::raw(" port bindings  "),
        Span::styled("q", Style::default().fg(theme.accent)),
        Span::raw(" quit"),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border));
    frame.render_widget(Paragraph::new(help).block(block), area);
}

fn draw_modal(frame: &mut Frame, app: &App, modal: &Modal, theme: &Theme) {
    let area = centered_rect(70, 70, frame.size());
    frame.render_widget(Clear, area);

    match modal {
        Modal::Create(form) => draw_create_modal(frame, form, theme, area),
        Modal::Restore(form) => draw_restore_modal(frame, form, theme, area),
        Modal::Bind(form) => draw_bind_modal(frame, form, theme, area),
        Modal::Sync(form) => draw_sync_modal(frame, form, theme, area),
        Modal::Mutagen(form) => draw_mutagen_modal(frame, app, form, theme, area),
        Modal::RemoteBrowser(form) => draw_remote_browser_modal(frame, form, theme, area),
        Modal::RsyncBind(form) => draw_rsync_bind_modal(frame, form, theme, area),
        Modal::RsyncBindActions(form) => draw_rsync_bind_actions_modal(frame, form, theme, area),
        Modal::DeleteRsyncBind(form) => draw_delete_rsync_bind_modal(frame, form, theme, area),
        Modal::Notice(notice) => draw_notice_modal(frame, notice, theme, area),
        Modal::Snapshot(form) => draw_snapshot_modal(frame, form, theme, area),
        Modal::Confirm(confirm) => draw_confirm_modal(frame, confirm, theme, area),
        Modal::Picker { picker, .. } => draw_picker_modal(frame, picker, theme, area),
    }
}

fn draw_create_modal(frame: &mut Frame, form: &CreateForm, theme: &Theme, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("Create Droplet")
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);

    let mut cursor = None;

    cursor =
        render_input_row(frame, "Name", &form.name, form.focus == 0, rows[0], theme).or(cursor);
    render_select_row(
        frame,
        "Region",
        form.region.as_ref().map(|s| s.label.as_str()),
        form.focus == 1,
        rows[1],
        theme,
    );
    render_select_row(
        frame,
        "Size",
        form.size.as_ref().map(|s| s.label.as_str()),
        form.focus == 2,
        rows[2],
        theme,
    );
    render_select_row(
        frame,
        "Image",
        form.image.as_ref().map(|s| s.label.as_str()),
        form.focus == 3,
        rows[3],
        theme,
    );
    let ssh_label = format!("{} selected", form.ssh_keys.len());
    render_select_row(
        frame,
        "SSH Keys",
        Some(ssh_label.as_str()),
        form.focus == 4,
        rows[4],
        theme,
    );
    cursor =
        render_input_row(frame, "Tags", &form.tags, form.focus == 5, rows[5], theme).or(cursor);
    render_action_row(frame, "Create", "Cancel", form.focus, 6, rows[6], theme);

    let help = Paragraph::new(Line::from(vec![
        Span::styled("Tab", Style::default().fg(theme.accent)),
        Span::raw(" move  "),
        Span::styled("Enter", Style::default().fg(theme.accent)),
        Span::raw(" select  "),
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::raw(" close"),
    ]))
    .style(Style::default().fg(theme.muted));
    frame.render_widget(help, rows[7]);

    if let Some((x, y)) = cursor {
        frame.set_cursor(x, y);
    }
}

fn draw_restore_modal(frame: &mut Frame, form: &RestoreForm, theme: &Theme, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("Restore Droplet")
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);

    let mut cursor = None;
    cursor =
        render_input_row(frame, "Name", &form.name, form.focus == 0, rows[0], theme).or(cursor);
    render_select_row(
        frame,
        "Snapshot",
        form.snapshot.as_ref().map(|s| s.label.as_str()),
        form.focus == 1,
        rows[1],
        theme,
    );
    render_select_row(
        frame,
        "Region",
        form.region.as_ref().map(|s| s.label.as_str()),
        form.focus == 2,
        rows[2],
        theme,
    );
    render_select_row(
        frame,
        "Size",
        form.size.as_ref().map(|s| s.label.as_str()),
        form.focus == 3,
        rows[3],
        theme,
    );
    let ssh_label = format!("{} selected", form.ssh_keys.len());
    render_select_row(
        frame,
        "SSH Keys",
        Some(ssh_label.as_str()),
        form.focus == 4,
        rows[4],
        theme,
    );
    cursor =
        render_input_row(frame, "Tags", &form.tags, form.focus == 5, rows[5], theme).or(cursor);
    render_action_row(frame, "Restore", "Cancel", form.focus, 6, rows[6], theme);

    let help = Paragraph::new(Line::from(vec![
        Span::styled("Tab", Style::default().fg(theme.accent)),
        Span::raw(" move  "),
        Span::styled("Enter", Style::default().fg(theme.accent)),
        Span::raw(" select  "),
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::raw(" close"),
    ]))
    .style(Style::default().fg(theme.muted));
    frame.render_widget(help, rows[7]);

    if let Some((x, y)) = cursor {
        frame.set_cursor(x, y);
    }
}

fn draw_bind_modal(frame: &mut Frame, form: &BindForm, theme: &Theme, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("Bind Local Port")
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);

    let mut cursor = None;
    let header = Paragraph::new(Line::from(vec![
        Span::styled(&form.droplet_name, Style::default().fg(theme.accent)),
        Span::raw(format!("  {}", form.public_ip)),
    ]))
    .style(Style::default());
    frame.render_widget(header, rows[0]);

    cursor = render_input_row(
        frame,
        "Local Port",
        &form.local_port,
        form.focus == 0,
        rows[1],
        theme,
    )
    .or(cursor);
    cursor = render_input_row(
        frame,
        "Remote Port",
        &form.remote_port,
        form.focus == 1,
        rows[2],
        theme,
    )
    .or(cursor);
    cursor = render_input_row(
        frame,
        "SSH User",
        &form.ssh_user,
        form.focus == 2,
        rows[3],
        theme,
    )
    .or(cursor);
    cursor = render_input_row(
        frame,
        "SSH Key",
        &form.ssh_key_path,
        form.focus == 3,
        rows[4],
        theme,
    )
    .or(cursor);
    cursor = render_input_row(
        frame,
        "SSH Port",
        &form.ssh_port,
        form.focus == 4,
        rows[5],
        theme,
    )
    .or(cursor);

    let action = Paragraph::new(Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent)),
        Span::raw(" bind  "),
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::raw(" cancel"),
    ]));
    frame.render_widget(action, rows[6]);

    if let Some((x, y)) = cursor {
        frame.set_cursor(x, y);
    }
}

fn draw_sync_modal(frame: &mut Frame, form: &SyncForm, theme: &Theme, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("Sync Folders (Mutagen)")
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(&form.droplet_name, Style::default().fg(theme.accent)),
        Span::raw(format!("  {}", form.public_ip)),
    ]))
    .style(Style::default());
    frame.render_widget(header, rows[0]);

    let mut cursor = None;
    cursor = render_input_row(
        frame,
        "Local Paths",
        &form.local_paths,
        form.focus == 0,
        rows[1],
        theme,
    )
    .or(cursor);
    cursor = render_input_row(
        frame,
        "SSH User",
        &form.ssh_user,
        form.focus == 1,
        rows[2],
        theme,
    )
    .or(cursor);
    cursor = render_input_row(
        frame,
        "SSH Key",
        &form.ssh_key_path,
        form.focus == 2,
        rows[3],
        theme,
    )
    .or(cursor);
    cursor = render_input_row(
        frame,
        "SSH Port",
        &form.ssh_port,
        form.focus == 3,
        rows[4],
        theme,
    )
    .or(cursor);

    render_action_row(frame, "Sync", "Cancel", form.focus, 4, rows[5], theme);

    let help = Paragraph::new(Line::from(vec![
        Span::styled("Comma-separated", Style::default().fg(theme.muted)),
        Span::raw("  use "),
        Span::styled("local->remote", Style::default().fg(theme.accent)),
        Span::raw(" to override remote path"),
    ]))
    .style(Style::default().fg(theme.muted));
    frame.render_widget(help, rows[6]);

    if let Some((x, y)) = cursor {
        frame.set_cursor(x, y);
    }
}

fn draw_mutagen_modal(
    frame: &mut Frame,
    app: &App,
    form: &crate::app::MutagenConfig,
    theme: &Theme,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("Mutagen Config")
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("Global", Style::default().fg(theme.muted)),
        Span::raw(" + "),
        Span::styled("Droplet", Style::default().fg(theme.muted)),
        Span::raw(" actions"),
    ]));
    frame.render_widget(header, rows[0]);

    let actions = app.mutagen_actions();
    let items: Vec<ListItem> = actions
        .iter()
        .map(|action| {
            let style = if action.enabled {
                Style::default().fg(theme.accent)
            } else {
                Style::default().fg(theme.muted)
            };
            ListItem::new(Line::from(vec![Span::styled(&action.label, style)]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Actions"))
        .highlight_style(
            Style::default()
                .bg(theme.accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = ratatui::widgets::ListState::default();
    if !actions.is_empty() {
        state.select(Some(form.selected.min(actions.len() - 1)));
    }
    frame.render_stateful_widget(list, rows[1], &mut state);

    let help = Paragraph::new(Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent)),
        Span::raw(" select  "),
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::raw(" close"),
    ]))
    .style(Style::default().fg(theme.muted));
    frame.render_widget(help, rows[2]);
}

fn draw_remote_browser_modal(
    frame: &mut Frame,
    form: &RemoteBrowserForm,
    theme: &Theme,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("Remote Folder Browser")
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(5),
        ])
        .split(inner);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(&form.droplet_name, Style::default().fg(theme.accent)),
        Span::raw("  "),
        Span::styled(&form.current_path, Style::default().fg(theme.muted)),
        if form.loading {
            Span::styled("  loading...", Style::default().fg(theme.warning))
        } else {
            Span::raw("")
        },
    ]));
    frame.render_widget(header, rows[0]);

    let items: Vec<ListItem> = if form.entries.is_empty() && !form.loading {
        vec![ListItem::new(Line::from(vec![Span::styled(
            "<no directories>",
            Style::default().fg(theme.muted),
        )]))]
    } else {
        form.entries
            .iter()
            .map(|entry| ListItem::new(Line::from(entry.label.clone())))
            .collect()
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Directories"))
        .highlight_style(
            Style::default()
                .bg(theme.accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = ratatui::widgets::ListState::default();
    if !form.entries.is_empty() {
        state.select(Some(form.selected.min(form.entries.len() - 1)));
    }
    frame.render_stateful_widget(list, rows[1], &mut state);

    let help = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Enter", Style::default().fg(theme.accent)),
            Span::raw(" open dir  "),
            Span::styled("Backspace", Style::default().fg(theme.accent)),
            Span::raw(" up  "),
            Span::styled("g", Style::default().fg(theme.accent)),
            Span::raw(" refresh"),
        ]),
        Line::from(vec![
            Span::styled("o", Style::default().fg(theme.accent)),
            Span::raw(" open highlighted in Cursor"),
        ]),
        Line::from(vec![
            Span::styled("m", Style::default().fg(theme.accent)),
            Span::raw(" bind rsync to local folder  "),
            Span::styled("Esc", Style::default().fg(theme.accent)),
            Span::raw(" close"),
        ]),
    ])
    .style(Style::default().fg(theme.muted))
    .wrap(Wrap { trim: true });
    frame.render_widget(help, rows[2]);
}

fn draw_rsync_bind_modal(frame: &mut Frame, form: &RsyncBindForm, theme: &Theme, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("Bind RSYNC to Local Folder")
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(&form.droplet_name, Style::default().fg(theme.accent)),
            Span::raw("  "),
            Span::styled(&form.remote_path, Style::default().fg(theme.muted)),
        ])),
        rows[0],
    );

    let cursor = render_input_row(
        frame,
        "Local Folder",
        &form.local_path,
        form.focus == 0,
        rows[1],
        theme,
    );
    render_action_row(
        frame,
        "Bind + Open Finder",
        "Cancel",
        form.focus,
        1,
        rows[2],
        theme,
    );

    let help = Paragraph::new(Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent)),
        Span::raw(" confirm  "),
        Span::styled("Tab", Style::default().fg(theme.accent)),
        Span::raw(" move  "),
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::raw(" close"),
    ]))
    .style(Style::default().fg(theme.muted));
    frame.render_widget(help, rows[3]);

    if let Some((x, y)) = cursor {
        frame.set_cursor(x, y);
    }
}

fn draw_rsync_bind_actions_modal(
    frame: &mut Frame,
    form: &RsyncBindActionsForm,
    theme: &Theme,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("RSYNC Bind Actions")
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);

    let info = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Droplet: ", Style::default().fg(theme.muted)),
            Span::styled(&form.bind.droplet_name, Style::default().fg(theme.accent)),
        ]),
        Line::from(vec![
            Span::styled("Remote:  ", Style::default().fg(theme.muted)),
            Span::raw(format!(
                "{}@{}:{}",
                form.bind.ssh_user, form.bind.host, form.bind.remote_path
            )),
        ]),
        Line::from(vec![
            Span::styled("Local:   ", Style::default().fg(theme.muted)),
            Span::raw(&form.bind.local_path),
        ]),
        Line::from(vec![
            Span::styled("SSH:     ", Style::default().fg(theme.muted)),
            Span::raw(format!(
                "key={}  port={}",
                form.bind.ssh_key_path, form.bind.ssh_port
            )),
        ]),
        Line::from(vec![
            Span::styled("Created: ", Style::default().fg(theme.muted)),
            Span::raw(
                form.bind
                    .created_at
                    .format("%Y-%m-%d %H:%M:%S UTC")
                    .to_string(),
            ),
        ]),
    ])
    .wrap(Wrap { trim: true });
    frame.render_widget(info, rows[0]);

    let action_button = |label: &str, active: bool| {
        if active {
            Span::styled(
                format!("[ {label} ]"),
                Style::default()
                    .bg(theme.accent)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(format!("[ {label} ]"), Style::default().fg(theme.muted))
        }
    };

    let sync_actions = Paragraph::new(Line::from(vec![
        Span::styled("Sync: ", Style::default().fg(theme.muted)),
        action_button("Push Up", form.selected_action == 0),
        Span::raw("  "),
        action_button("Pull Down", form.selected_action == 1),
    ]));
    frame.render_widget(sync_actions, rows[1]);

    let other_actions = Paragraph::new(Line::from(vec![
        Span::styled("More: ", Style::default().fg(theme.muted)),
        action_button("Open Finder", form.selected_action == 2),
        Span::raw("  "),
        action_button("Open iTerm", form.selected_action == 3),
        Span::raw("  "),
        action_button("Delete Bind", form.selected_action == 4),
        Span::raw("  "),
        action_button("Close", form.selected_action == 5),
    ]))
    .wrap(Wrap { trim: true });
    frame.render_widget(other_actions, rows[2]);

    let help = Paragraph::new(Line::from(vec![
        Span::styled("Left/Right", Style::default().fg(theme.accent)),
        Span::raw(" select  "),
        Span::styled("Enter", Style::default().fg(theme.accent)),
        Span::raw(" run action  "),
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::raw(" close"),
    ]))
    .style(Style::default().fg(theme.muted));
    frame.render_widget(help, rows[3]);
}

fn draw_delete_rsync_bind_modal(
    frame: &mut Frame,
    form: &DeleteRsyncBindForm,
    theme: &Theme,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("Delete RSYNC Bind")
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);

    let summary = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Remote: ", Style::default().fg(theme.muted)),
            Span::raw(format!(
                "{}@{}:{}",
                form.bind.ssh_user, form.bind.host, form.bind.remote_path
            )),
        ]),
        Line::from(vec![
            Span::styled("Local:  ", Style::default().fg(theme.muted)),
            Span::raw(&form.bind.local_path),
        ]),
    ])
    .wrap(Wrap { trim: true });
    frame.render_widget(summary, rows[0]);

    let checkbox = if form.delete_local_copy { "[x]" } else { "[ ]" };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(checkbox, Style::default().fg(theme.accent)),
            Span::raw(" Also delete local copy"),
        ])),
        rows[1],
    );

    let help = Paragraph::new(Line::from(vec![
        Span::styled("Space", Style::default().fg(theme.accent)),
        Span::raw(" toggle  "),
        Span::styled("Enter", Style::default().fg(theme.accent)),
        Span::raw(" delete  "),
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::raw(" cancel"),
    ]))
    .style(Style::default().fg(theme.muted));
    frame.render_widget(help, rows[2]);
}

fn draw_notice_modal(frame: &mut Frame, notice: &Notice, theme: &Theme, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(notice.title.as_str())
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(notice.message.clone()).wrap(Wrap { trim: true }),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Enter", Style::default().fg(theme.accent)),
            Span::raw(" close  "),
            Span::styled("Esc", Style::default().fg(theme.accent)),
            Span::raw(" close"),
        ]))
        .style(Style::default().fg(theme.muted)),
        rows[1],
    );
}

fn draw_snapshot_modal(frame: &mut Frame, form: &SnapshotForm, theme: &Theme, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title("Snapshot + Delete")
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(&form.droplet_name, Style::default().fg(theme.accent)),
        Span::raw(" will be snapshot and deleted"),
    ]));
    frame.render_widget(header, rows[0]);

    let cursor = render_input_row(
        frame,
        "Snapshot Name",
        &form.snapshot_name,
        true,
        rows[1],
        theme,
    );

    let help = Paragraph::new(Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent)),
        Span::raw(" continue  "),
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::raw(" cancel"),
    ]));
    frame.render_widget(help, rows[2]);

    if let Some((x, y)) = cursor {
        frame.set_cursor(x, y);
    }
}

fn draw_confirm_modal(frame: &mut Frame, confirm: &crate::app::Confirm, theme: &Theme, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(confirm.title.as_str())
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(inner);

    let content = Paragraph::new(confirm.message.clone()).wrap(Wrap { trim: true });
    frame.render_widget(content, rows[0]);

    let help = Paragraph::new(Line::from(vec![
        Span::styled("y", Style::default().fg(theme.success)),
        Span::raw(" confirm  "),
        Span::styled("n", Style::default().fg(theme.warning)),
        Span::raw(" cancel"),
    ]));
    frame.render_widget(help, rows[1]);
}

fn draw_picker_modal(frame: &mut Frame, picker: &Picker, theme: &Theme, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(picker.title.as_str())
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);

    let label = "Search: ";
    let query = Paragraph::new(Line::from(vec![
        Span::styled(label, Style::default().fg(theme.muted)),
        Span::styled(&picker.query.value, Style::default().fg(Color::White)),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Filter"));
    frame.render_widget(query, rows[0]);
    let cursor_x = rows[0].x + 1 + label.len() as u16 + picker.query.cursor_display_offset() as u16;
    let cursor_y = rows[0].y + 1;
    frame.set_cursor(cursor_x, cursor_y);

    let items: Vec<ListItem> = picker
        .filtered
        .iter()
        .filter_map(|idx| picker.items.get(*idx))
        .map(|item| {
            let marker = if picker.multi {
                if picker.chosen.iter().any(|chosen| {
                    picker
                        .items
                        .get(*chosen)
                        .map(|i| i.value == item.value)
                        .unwrap_or(false)
                }) {
                    "[x]"
                } else {
                    "[ ]"
                }
            } else {
                "   "
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(theme.muted)),
                Span::raw(" "),
                Span::raw(&item.label),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .bg(theme.accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = ratatui::widgets::ListState::default();
    if !picker.filtered.is_empty() {
        state.select(Some(picker.selected.min(picker.filtered.len() - 1)));
    }
    frame.render_stateful_widget(list, rows[1], &mut state);

    let help = Paragraph::new(Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent)),
        Span::raw(" select  "),
        if picker.multi {
            Span::styled("Space", Style::default().fg(theme.accent))
        } else {
            Span::raw(" ")
        },
        if picker.multi {
            Span::raw(" toggle  ")
        } else {
            Span::raw(" ")
        },
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::raw(" back"),
    ]));
    frame.render_widget(help, rows[2]);
}

fn render_input_row(
    frame: &mut Frame,
    label: &str,
    input: &TextInput,
    focused: bool,
    area: Rect,
    theme: &Theme,
) -> Option<(u16, u16)> {
    let style = if focused {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.muted)
    };

    let line = Line::from(vec![
        Span::styled(format!("{label}: "), style),
        Span::raw(&input.value),
    ]);
    frame.render_widget(Paragraph::new(line), area);

    if focused {
        let cursor_x = area.x + label.len() as u16 + 2 + input.cursor_display_offset() as u16;
        let cursor_y = area.y;
        Some((cursor_x, cursor_y))
    } else {
        None
    }
}

fn render_select_row(
    frame: &mut Frame,
    label: &str,
    value: Option<&str>,
    focused: bool,
    area: Rect,
    theme: &Theme,
) {
    let style = if focused {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.muted)
    };
    let value = value.unwrap_or("<select>");
    let line = Line::from(vec![
        Span::styled(format!("{label}: "), style),
        Span::raw(value),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_action_row(
    frame: &mut Frame,
    primary: &str,
    secondary: &str,
    focus: usize,
    submit_index: usize,
    area: Rect,
    theme: &Theme,
) {
    let submit_style = if focus == submit_index {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.muted)
    };
    let cancel_style = if focus == submit_index + 1 {
        Style::default()
            .fg(theme.warning)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.muted)
    };
    let line = Line::from(vec![
        Span::styled(format!("[ {primary} ]"), submit_style),
        Span::raw("  "),
        Span::styled(format!("[ {secondary} ]"), cancel_style),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_toast(frame: &mut Frame, app: &App, theme: &Theme) {
    let toast = match &app.toast {
        Some(toast) => toast,
        None => return,
    };
    if (Utc::now() - toast.created_at).num_seconds() > 6 {
        return;
    }
    let style = match toast.level {
        ToastLevel::Info => Style::default().fg(theme.muted),
        ToastLevel::Success => Style::default().fg(theme.success),
        ToastLevel::Warning => Style::default().fg(theme.warning),
        ToastLevel::Error => Style::default().fg(theme.error),
    };
    let area = frame.size();
    let rect = Rect {
        x: area.x + 2,
        y: area.y + area.height.saturating_sub(4),
        width: area.width.saturating_sub(4),
        height: 1,
    };
    frame.render_widget(Paragraph::new(toast.message.clone()).style(style), rect);
}

fn draw_loading_overlay(frame: &mut Frame, app: &App, theme: &Theme) {
    if app.pending == 0 {
        return;
    }

    let frames = ["|", "/", "-", "\\"];
    let spinner_idx = ((Utc::now().timestamp_subsec_millis() / 120) % frames.len() as u32) as usize;
    let spinner = frames[spinner_idx];

    let area = centered_rect(64, 34, frame.size());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title("Working")
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let inner = inner_rect(area, 1);
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(spinner, Style::default().fg(theme.accent)),
        Span::raw(" "),
        Span::styled("Please wait...", Style::default().fg(theme.accent)),
    ]));
    lines.push(Line::from(""));
    for line in app.pending_overlay_lines() {
        lines.push(Line::from(line));
    }

    let content = Paragraph::new(lines)
        .style(Style::default().fg(theme.muted))
        .wrap(Wrap { trim: true });
    frame.render_widget(content, inner);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn inner_rect(area: Rect, margin: u16) -> Rect {
    Rect {
        x: area.x + margin,
        y: area.y + margin,
        width: area.width.saturating_sub(margin * 2),
        height: area.height.saturating_sub(margin * 2),
    }
}

fn app_state_list(app: &App) -> ratatui::widgets::ListState {
    let mut state = ratatui::widgets::ListState::default();
    let max = app.visible_indices().len();
    if max > 0 {
        let selected = app.selected.min(max - 1);
        state.select(Some(selected));
    }
    state
}

fn binding_state_list(app: &App) -> ratatui::widgets::ListState {
    let mut state = ratatui::widgets::ListState::default();
    let max = app.state.bindings.len();
    if max > 0 {
        let selected = app.selected.min(max - 1);
        state.select(Some(selected));
    }
    state
}

fn rsync_bind_state_list(app: &App) -> ratatui::widgets::ListState {
    let mut state = ratatui::widgets::ListState::default();
    let max = app.state.rsync_binds.len();
    if max > 0 {
        let selected = app.selected.min(max - 1);
        state.select(Some(selected));
    }
    state
}
