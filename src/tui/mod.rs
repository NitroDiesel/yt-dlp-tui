mod app;
mod form;
pub mod format;
mod new_download;
mod settings;
mod text_input;
mod theme;
mod view;

use std::{io::stdout, sync::Arc, time::Duration};

use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste, Event},
    execute,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::{
    application::{AppService, ServiceEvent},
    domain::{AppSnapshot, HardwareAccelerationInfo},
    error::AppResult,
};

pub use app::App;

/// How often the reader thread checks whether the interface has closed.
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// Redraw cadence while nothing else happens, for notices that expire.
const TICK: Duration = Duration::from_millis(500);

/// Runs the interface until the user quits, then stops running downloads so
/// they can resume next time. `initial_link` opens New download for it.
pub async fn run(
    service: Arc<AppService>,
    mut service_events: UnboundedReceiver<ServiceEvent>,
    initial_link: Option<String>,
) -> AppResult<()> {
    let (queue, history) = service.jobs().await?;
    let snapshot = AppSnapshot {
        settings: service.settings().await,
        queue,
        history,
        // Tool checks start yt-dlp, which takes a moment; they finish in the background.
        dependencies: Vec::new(),
        hardware_acceleration: HardwareAccelerationInfo::checking(),
        queue_paused: service.queue_paused(),
    };
    let (tx, mut messages) = unbounded_channel();
    let mut app = App::new(service.clone(), snapshot, tx);
    app.start(initial_link);

    let mut terminal = enter_terminal()?;
    let (event_tx, mut events) = unbounded_channel();
    let reader = spawn_reader(event_tx);
    let mut ticker = tokio::time::interval(TICK);
    let result = loop {
        if let Err(error) = terminal.draw(|frame| view::render(frame, &mut app)) {
            break Err(error.into());
        }
        tokio::select! {
            Some(event) = events.recv() => app.handle_event(event),
            Some(message) = messages.recv() => app.handle_message(message),
            Some(event) = service_events.recv() => app.handle_service(event),
            _ = ticker.tick() => {
                app.tick();
            }
        }
        if app.should_quit {
            break Ok(());
        }
    };
    drop(events);
    let _ = reader.join();
    leave_terminal();
    let running = service.running_count().await;
    if running > 0 {
        eprintln!(
            "Stopping {running} {}. Retry resumes from the partial file.",
            if running == 1 {
                "download"
            } else {
                "downloads"
            }
        );
    }
    service.shutdown().await;
    result
}

fn enter_terminal() -> AppResult<ratatui::DefaultTerminal> {
    let terminal = ratatui::init();
    // ratatui's hook restores raw mode and the screen; bracketed paste is ours.
    let restore = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableBracketedPaste);
        restore(info);
    }));
    execute!(stdout(), EnableBracketedPaste)?;
    Ok(terminal)
}

fn leave_terminal() {
    let _ = execute!(stdout(), DisableBracketedPaste);
    ratatui::restore();
}

/// Reads terminal input on its own thread. It polls rather than blocking so
/// it notices within one interval that the interface has shut down.
fn spawn_reader(tx: UnboundedSender<Event>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !tx.is_closed() {
            match crossterm::event::poll(POLL_INTERVAL) {
                Ok(true) => match crossterm::event::read() {
                    Ok(event) => {
                        if tx.send(event).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Ok(false) => {}
                Err(_) => break,
            }
        }
    })
}
