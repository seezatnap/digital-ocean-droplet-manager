mod app;
mod config;
mod doctl;
mod input;
mod model;
mod ports;
mod tasks;
mod ui;

use std::time::{Duration, Instant};

use crossbeam_channel::unbounded;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::app::App;

fn main() -> anyhow::Result<()> {
    let (tx, rx) = unbounded();
    let mut app = App::new(tx.clone());
    app.bootstrap();

    let mut terminal = ui::setup_terminal()?;
    let tick_rate = Duration::from_millis(120);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        app.should_quit = true;
                    } else {
                        app.handle_key(key);
                    }
                }
            }
        }

        while let Ok(message) = rx.try_recv() {
            app.handle_task_result(message);
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    app.shutdown();
    ui::restore_terminal(terminal)?;
    Ok(())
}
