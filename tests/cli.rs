use std::process::{Command, Stdio};

/// `yt-dlp-tui --help | head -1` closes the pipe early; that must not panic.
#[test]
fn printing_to_a_closed_pipe_exits_cleanly() {
    for argument in ["--help", "--version", "paths"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_yt-dlp-tui"))
            .arg(argument)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{argument}: {:?}", output.status);
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("panicked"),
            "{argument} panicked"
        );
    }
}
