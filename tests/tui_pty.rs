//! Drives the real binary in a pseudo-terminal against the fake engine and
//! reads the screen through a terminal emulator, the way a person would.
//! Run with `--nocapture` to print each screen.

use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, channel},
    time::{Duration, Instant},
};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

const ROWS: u16 = 36;
const COLUMNS: u16 = 120;

struct Session {
    _master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    screen: vt100::Parser,
}

impl Session {
    fn start(home: &Path, tools: &Path) -> Self {
        Self::launch(home, tools, None, &[])
    }

    /// Starts the app with `extra_path` ahead of the system folders on
    /// `PATH`, and `args` on its command line.
    fn launch(home: &Path, tools: &Path, extra_path: Option<&Path>, args: &[&str]) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLUMNS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_yt-dlp-tui"));
        command.args(args);
        command.env_clear();
        command.env("HOME", home);
        command.env("TERM", "xterm-256color");
        let path = match extra_path {
            Some(extra) => format!("{}:/usr/bin:/bin", extra.display()),
            None => "/usr/bin:/bin".into(),
        };
        command.env("PATH", path);
        command.env("YT_DLP_TUI_DATA_DIR", home.join("data"));
        command.env("YT_DLP_TUI_TOOLS_DIR", tools);
        command.cwd(home);
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let (sender, output) = channel();
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 8192];
            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 || sender.send(buffer[..read].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            _master: pair.master,
            child,
            writer,
            output,
            screen: vt100::Parser::new(ROWS, COLUMNS, 0),
        }
    }

    fn contents(&self) -> String {
        self.screen.screen().contents()
    }

    fn pump(&mut self, wait: Duration) {
        if let Ok(bytes) = self.output.recv_timeout(wait) {
            self.screen.process(&bytes);
            while let Ok(bytes) = self.output.try_recv() {
                self.screen.process(&bytes);
            }
        }
    }

    fn wait_for(&mut self, text: &str) {
        self.wait_for_within(text, Duration::from_secs(20));
    }

    fn wait_for_within(&mut self, text: &str, limit: Duration) {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if self.contents().contains(text) {
                return;
            }
            self.pump(Duration::from_millis(50));
        }
        panic!(
            "timed out waiting for {text:?}. Screen:\n{}",
            self.contents()
        );
    }

    /// Screen row and column of the first occurrence of `text`.
    fn position_of(&self, text: &str) -> Option<(u16, u16)> {
        self.contents().lines().enumerate().find_map(|(row, line)| {
            line.find(text)
                .map(|byte| (row as u16, line[..byte].chars().count() as u16))
        })
    }

    /// Clicks the middle of `text` with the left button, as a terminal
    /// reports it in SGR mouse mode.
    fn click(&mut self, text: &str) {
        self.wait_for(text);
        let (row, column) = self.position_of(text).unwrap();
        let column = column + text.chars().count() as u16 / 2;
        let (x, y) = (column + 1, row + 1);
        self.send(&format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m"));
    }

    fn wait_until_gone(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if !self.contents().contains(text) {
                return;
            }
            self.pump(Duration::from_millis(50));
        }
        panic!(
            "timed out waiting for {text:?} to go away. Screen:\n{}",
            self.contents()
        );
    }

    /// Terminals send Alt+key as Escape followed by the key, so a key sent
    /// right after Escape can merge with it into Alt+key. After Escape, wait
    /// for a screen state that proves it was handled.
    fn send(&mut self, bytes: &str) {
        self.writer.write_all(bytes.as_bytes()).unwrap();
        self.writer.flush().unwrap();
        self.pump(Duration::from_millis(150));
    }

    fn snapshot(&mut self, title: &str) {
        self.pump(Duration::from_millis(200));
        println!("──── {title} ────\n{}\n", self.contents());
    }
}

fn fake_tools() -> tempfile::TempDir {
    let tools = tempfile::tempdir().unwrap();
    let engine = PathBuf::from(env!("CARGO_BIN_EXE_fake-ytdlp"));
    std::fs::copy(&engine, tools.path().join("yt-dlp")).unwrap();
    tools
}

#[test]
fn pastes_a_link_downloads_it_and_navigates_the_app() {
    let home = tempfile::tempdir().unwrap();
    let tools = fake_tools();
    let mut session = Session::start(home.path(), tools.path());

    session.wait_for("Paste a link to start");
    session.wait_for("FFmpeg missing");
    assert!(session.contents().contains("Pinned tools to install"));
    session.snapshot("First launch");

    // Pasting a link anywhere opens New download and reads it.
    session.send("\x1b[200~watch https://example.test/slow now\x1b[201~");
    session.wait_for("Fixture channel on example.test");
    session.wait_for("Add to queue");
    session.snapshot("Link analyzed");

    session.send("\x04");
    session.wait_for("Downloading");
    session.wait_for("MB/s");
    session.snapshot("Downloading");
    session.wait_for("Completed 1  Stopped");
    session.snapshot("Completed");
    let saved = home.path().join("Downloads").join("Fixture media.webm");
    assert!(saved.is_file(), "the download lands in ~/Downloads");

    session.send("?");
    session.wait_for("Move a waiting download up or down");
    session.snapshot("Help");
    session.send("x");

    session.send("s");
    session.wait_for("Download engine");
    session.wait_for("2026.08.19");
    session.snapshot("Settings");
    session.send("\x1b");
    session.wait_for("n new");

    session.send("n");
    session.wait_for("Paste a video, playlist, or channel link");
    session.send("https://example.test/failure\r");
    session.wait_for("Couldn't read that link");
    session.snapshot("Analysis failed");
    session.send("\x1b");
    session.wait_for("n new");

    session.send("q");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = session.child.try_wait().unwrap() {
            assert!(status.success(), "quits cleanly: {status:?}");
            break;
        }
        assert!(Instant::now() < deadline, "the app quits");
        session.pump(Duration::from_millis(50));
    }
}

fn wait_for_exit(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = session.child.try_wait().unwrap() {
            assert!(status.success(), "quits cleanly: {status:?}");
            return;
        }
        assert!(Instant::now() < deadline, "the app quits");
        session.pump(Duration::from_millis(50));
    }
}

const DOWN: &str = "\x1b[B";
const RIGHT: &str = "\x1b[C";

#[test]
fn saves_settings_that_survive_a_restart() {
    let home = tempfile::tempdir().unwrap();
    let tools = fake_tools();
    let mut session = Session::start(home.path(), tools.path());
    session.wait_for("Paste a link to start");
    session.send("s");
    session.wait_for("Downloads at once");
    // Default folder, File name, Default type, Default quality, Downloads at once.
    for _ in 0..4 {
        session.send(DOWN);
    }
    session.send(RIGHT);
    session.wait_for("You have unsaved changes");
    session.send("\x13");
    session.wait_for("Settings saved");
    session.send("\x1b");
    session.wait_for("Paste a link to start");
    session.send("q");
    wait_for_exit(&mut session);

    let mut session = Session::start(home.path(), tools.path());
    session.wait_for("Paste a link to start");
    session.send("s");
    session.wait_for("Downloads at once    2");
    session.send("\x1b");
    session.wait_for("Paste a link to start");
    session.send("q");
    wait_for_exit(&mut session);
}

#[test]
fn confirms_whole_playlists_and_offers_current_sponsorblock_segments() {
    let home = tempfile::tempdir().unwrap();
    let tools = fake_tools();
    let mut session = Session::start(home.path(), tools.path());
    session.wait_for("Paste a link to start");

    session.send("\x1b[200~https://example.test/video\x1b[201~");
    session.wait_for("Add to queue");
    session.wait_for("Show advanced options");
    // Kind, Quality, Container, Video codec, Clip, Save to, Advanced.
    for _ in 0..6 {
        session.send(DOWN);
    }
    session.send("\r");
    session.wait_for("Hide advanced options");
    // Subtitles (2 rows), Chapters (3 rows), then SponsorBlock.
    for _ in 0..6 {
        session.send(DOWN);
    }
    session.send(RIGHT);
    session.wait_for("Mark segments as chapters");
    session.send(DOWN);
    session.send("\r");
    session.wait_for("Hooks and greetings");
    session.wait_for("Highlight (mark only)");
    session.snapshot("SponsorBlock segments");
    session.send("\x1b");
    session.wait_until_gone("Space check");
    session.send("\x1b");
    session.wait_for("n new");

    session.send("\x1b[200~https://example.test/playlist\x1b[201~");
    session.wait_for("Playlist of 3");
    session.send("\x04");
    session.wait_for("This will download all 3 items. Continue?");
    session.snapshot("Playlist confirmation");
    session.send("\r");
    session.wait_for("Fixture playlist");
    session.wait_for("Completed 1  Stopped");
    session.send("q");
    wait_for_exit(&mut session);
}

#[test]
fn quitting_mid_download_leaves_it_ready_to_resume() {
    let home = tempfile::tempdir().unwrap();
    let tools = fake_tools();
    let mut session = Session::start(home.path(), tools.path());
    session.wait_for("Paste a link to start");
    session.send("\x1b[200~https://example.test/cancel\x1b[201~");
    session.wait_for("Add to queue");
    session.send("\x04");
    session.wait_for("Active 1");
    session.send("q");
    session.wait_for("1 download is running and will stop");
    session.snapshot("Quit confirmation");
    session.send("\r");
    wait_for_exit(&mut session);

    let mut session = Session::start(home.path(), tools.path());
    session.wait_for("Stopped 1");
    session.wait_for("Interrupted");
    session.wait_for("r retry");
    session.send("q");
    wait_for_exit(&mut session);
}

#[test]
fn a_fresh_machine_sets_up_its_own_engine_and_ignores_yt_dlp_on_path() {
    let home = tempfile::tempdir().unwrap();
    let empty_tools = tempfile::tempdir().unwrap();
    // A yt-dlp on PATH must never be picked up.
    let system = fake_tools();
    let mut session = Session::launch(home.path(), empty_tools.path(), Some(system.path()), &[]);

    session.wait_for("Set up the download engine?");
    session.wait_for("so you don't need yt-dlp,");
    session.snapshot("First launch on a fresh machine");
    session.send("\x1b");
    session.wait_for("The download engine isn't set up yet. Press I");
    session.wait_for("Engine not set up");
    session.wait_for("I set up engine");

    // A link pasted before setup asks for setup instead of failing.
    session.send("\x1b[200~https://example.test/video\x1b[201~");
    session.wait_for("isn't set up yet. Press Enter to install");
    session.wait_for("Set up the download engine?");
    session.send("\x1b");
    session.wait_until_gone("Set up the download engine?");
    session.send("\x1b");
    session.wait_for("n new");
    session.send("q");
    wait_for_exit(&mut session);
}

#[test]
fn a_link_given_at_launch_is_read_once_the_engine_check_finishes() {
    let home = tempfile::tempdir().unwrap();
    let tools = fake_tools();
    let mut session = Session::launch(
        home.path(),
        tools.path(),
        None,
        &["https://example.test/video"],
    );
    session.wait_for("Fixture channel on example.test");
    session.wait_for("Add to queue");
    session.send("\x1b");
    session.wait_for("n new");
    session.send("q");
    wait_for_exit(&mut session);
}

#[test]
fn works_with_the_mouse() {
    let home = tempfile::tempdir().unwrap();
    let tools = fake_tools();
    let mut session = Session::start(home.path(), tools.path());
    session.wait_for("Paste a link to start");
    assert_ne!(
        session.screen.screen().mouse_protocol_mode(),
        vt100::MouseProtocolMode::None,
        "the app asks the terminal for mouse events"
    );

    session.click("+ New download");
    session.wait_for("Paste a video, playlist, or channel link");
    session.send("\x1b[200~https://example.test/video\x1b[201~");
    session.wait_for("Add to queue");
    // A choice opens its picker, and an option is chosen by clicking it.
    session.click("Best available");
    session.wait_for("Enter choose");
    session.click("Up to 720p");
    session.wait_until_gone("Enter choose");
    session.wait_for("Up to 720p");
    session.snapshot("Quality picked with the mouse");
    session.click("[ Download ]");
    session.wait_for("Completed 1  Stopped");

    // A row selects its download; the details offer buttons.
    session.click("Fixture media");
    session.wait_for("[ Copy link ]");
    session.wait_for("[ Show in folder ]");
    session.snapshot("Details with buttons");
    session.click("[ Remove ]");
    session.wait_for("Paste a link to start");

    session.click("Settings");
    session.wait_for("Download engine");
    session.click("Date files by download time");
    session.wait_for("You have unsaved changes");
    session.click("Ctrl+S save");
    session.wait_for("Settings saved");
    session.click("All downloads");
    session.wait_for("Paste a link to start");

    // Key hints are buttons too.
    session.click("? help");
    session.wait_for("Move a waiting download up or down");
    session.click("Move a waiting");
    session.wait_until_gone("Move a waiting download up or down");

    session.send("\x1b[200~https://example.test/cancel\x1b[201~");
    session.click("[ Download ]");
    session.wait_for("Active 1");
    session.click("q quit");
    session.wait_for("Quit yt-dlp TUI?");
    session.click("[ Keep running ]");
    session.wait_until_gone("Quit yt-dlp TUI?");
    session.click("q quit");
    session.click("[ Quit ]");
    wait_for_exit(&mut session);
}

#[test]
fn hover_highlights_and_the_mouse_can_be_turned_off() {
    let home = tempfile::tempdir().unwrap();
    let tools = fake_tools();
    let mut session = Session::start(home.path(), tools.path());
    session.wait_for("Paste a link to start");

    // Pointer movement (button 35 = motion) underlines the hint under it.
    session.wait_for("? help");
    let (row, column) = session.position_of("? help").unwrap();
    session.send(&format!("\x1b[<35;{};{}M", column + 2, row + 1));
    let underlined = |session: &Session| {
        session
            .screen
            .screen()
            .cell(row, column)
            .is_some_and(|cell| cell.underline())
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while !underlined(&session) && Instant::now() < deadline {
        session.pump(Duration::from_millis(50));
    }
    assert!(underlined(&session), "the hovered hint is underlined");

    // The wheel scrolls Settings down to the mouse switch.
    session.click("Settings");
    session.wait_for("Download engine");
    let (row, column) = session.position_of("Download engine").unwrap();
    for _ in 0..40 {
        session.send(&format!("\x1b[<65;{};{}M", column + 2, row + 1));
    }
    session.click("Click, scroll, and hover");
    session.send("\x13");
    session.wait_for("Settings saved");
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.screen.screen().mouse_protocol_mode() != vt100::MouseProtocolMode::None
        && Instant::now() < deadline
    {
        session.pump(Duration::from_millis(50));
    }
    assert_eq!(
        session.screen.screen().mouse_protocol_mode(),
        vt100::MouseProtocolMode::None,
        "turning the mouse off releases it to the terminal"
    );
    session.send("\x1b");
    session.wait_for("Paste a link to start");
    session.send("q");
    wait_for_exit(&mut session);
}

#[test]
fn refuses_to_run_twice_on_the_same_data() {
    let home = tempfile::tempdir().unwrap();
    let tools = fake_tools();
    let mut first = Session::start(home.path(), tools.path());
    first.wait_for("Paste a link to start");
    let mut second = Session::start(home.path(), tools.path());
    second.wait_for("already running in another terminal");
    first.send("q");
}

/// The whole path with the real pinned tools and network: see
/// `tests/live_download.rs` for how to install them and set the variable.
#[test]
#[ignore = "downloads from YouTube with the installed pinned tools"]
fn downloads_through_the_interface_with_the_pinned_engine() {
    let tools = PathBuf::from(
        std::env::var_os("YT_DLP_TUI_LIVE_TOOLS")
            .expect("set YT_DLP_TUI_LIVE_TOOLS to the installed tools folder"),
    );
    let home = tempfile::tempdir().unwrap();
    let mut session = Session::start(home.path(), &tools);
    session.wait_for("Engine ready");
    session.send("\x1b[200~https://www.youtube.com/watch?v=jNQXAC9IVRw\x1b[201~");
    session.wait_for("jawed on youtube.com");
    session.wait_for("Add to queue");
    session.snapshot("Analyzed with yt-dlp 2026.08.19");
    session.send("\x04");
    session.wait_for("Completed 1  Stopped");
    session.snapshot("Downloaded");
    let saved = std::fs::read_dir(home.path().join("Downloads"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(
        saved.iter().any(|name| name.starts_with("Me at the zoo")),
        "{saved:?}"
    );
    session.send("q");
    wait_for_exit(&mut session);
}

/// A machine with no yt-dlp, Deno, or app tools: the app installs its own
/// engine at first launch, then reads the link it was started with.
#[test]
#[ignore = "downloads the pinned tools (about 200 MB) and a video from YouTube"]
fn a_fresh_machine_downloads_with_nothing_preinstalled() {
    let home = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let mut session = Session::launch(
        home.path(),
        tools.path(),
        None,
        &["https://www.youtube.com/watch?v=jNQXAC9IVRw"],
    );
    session.wait_for("Set up the download engine?");
    session.snapshot("Fresh machine, first launch with a link");
    session.send("\r");
    session.wait_for("Setting up engine");
    session.wait_for_within("Engine ready", Duration::from_secs(600));
    session.wait_for("jawed on youtube.com");
    session.wait_for("Add to queue");
    session.snapshot("Engine installed and link read");
    session.send("\x04");
    session.wait_for_within("Completed 1  Stopped", Duration::from_secs(120));
    session.snapshot("Downloaded");
    assert!(tools.path().join("installed.json").is_file());
    let saved = std::fs::read_dir(home.path().join("Downloads"))
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("Me at the zoo")
        });
    assert!(saved, "the video is in ~/Downloads");
    session.send("q");
    wait_for_exit(&mut session);
}
