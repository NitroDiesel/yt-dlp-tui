//! Converts real clips into MOV, AVI, and WebM with the installed FFmpeg and
//! FFprobe, and checks each result plays back with the expected codecs. Runs
//! on request:
//!
//! ```text
//! YT_DLP_TUI_LIVE_TOOLS="$(cargo run -q -- paths | sed -n 's/^Tools: //p')" \
//!     cargo test --test container_conversion -- --ignored --nocapture
//! ```

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use tokio_util::sync::CancellationToken;
use yt_dlp_tui::{
    domain::VideoContainer,
    integration::container::{parse_probe, run_container_conversion},
};

fn tool(name: &str) -> PathBuf {
    PathBuf::from(
        std::env::var_os("YT_DLP_TUI_LIVE_TOOLS")
            .expect("set YT_DLP_TUI_LIVE_TOOLS to the installed tools folder"),
    )
    .join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    })
}

/// A two-second clip with the given codecs, an English title, and subtitles.
fn clip(folder: &Path, name: &str, video: &[&str], audio: &str) -> PathBuf {
    let subtitles = folder.join("subtitles.srt");
    std::fs::write(&subtitles, "1\n00:00:00,000 --> 00:00:01,500\nHello\n").unwrap();
    let path = folder.join(name);
    // Each fixture carries the subtitle format its own container allows.
    let subtitle_codec = match path.extension().and_then(|extension| extension.to_str()) {
        Some("mp4") => "mov_text",
        Some("webm") => "webvtt",
        _ => "ass",
    };
    let made = Command::new(tool("ffmpeg"))
        .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=320x240:rate=30:duration=2")
        .args(["-f", "lavfi", "-i", "sine=duration=2", "-i"])
        .arg(&subtitles)
        .args(["-map", "0", "-map", "1", "-map", "2", "-c:v"])
        .args(video)
        .args([
            "-c:a",
            audio,
            "-c:s",
            subtitle_codec,
            "-metadata",
            "title=Fixture",
        ])
        .arg(&path)
        .status()
        .unwrap();
    assert!(made.success(), "FFmpeg made {name}");
    path
}

fn codecs(path: &Path) -> (Vec<String>, f64) {
    let output = Command::new(tool("ffprobe"))
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=index,codec_type,codec_name:stream_disposition=attached_pic:format=duration",
            "-of",
            "json",
            "-i",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let (streams, duration) = parse_probe(&output.stdout).unwrap();
    (
        streams.into_iter().map(|stream| stream.codec).collect(),
        duration.unwrap_or_default(),
    )
}

async fn convert(input: &Path, container: VideoContainer) -> (PathBuf, Vec<String>) {
    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let outcome = tokio::time::timeout(
        Duration::from_secs(120),
        run_container_conversion(
            &tool("ffmpeg"),
            &tool("ffprobe"),
            input,
            container,
            CancellationToken::new(),
            events,
        ),
    )
    .await
    .expect("the conversion finishes")
    .expect("the conversion succeeds");
    assert!(!outcome.cancelled);
    assert!(!input.exists(), "the original is replaced");
    let decoded = Command::new(tool("ffmpeg"))
        .args(["-v", "error", "-i"])
        .arg(&outcome.output_path)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decoded.status.success() && decoded.stderr.is_empty(),
        "{} decodes cleanly: {}",
        outcome.output_path.display(),
        String::from_utf8_lossy(&decoded.stderr)
    );
    (outcome.output_path, outcome.diagnostics)
}

#[tokio::test]
#[ignore = "needs the installed pinned tools"]
async fn h264_and_aac_are_copied_into_mov() {
    let folder = tempfile::tempdir().unwrap();
    let input = clip(folder.path(), "clip.mp4", &["libx264"], "aac");
    let (output, notes) = convert(&input, VideoContainer::Mov).await;
    assert_eq!(
        output,
        folder.path().join("clip.mov").canonicalize().unwrap()
    );
    let (codecs, duration) = codecs(&output);
    assert_eq!(codecs, ["h264", "aac", "mov_text"]);
    assert!((duration - 2.0).abs() < 0.2);
    assert!(notes.is_empty(), "nothing re-encoded: {notes:?}");
}

#[tokio::test]
#[ignore = "needs the installed pinned tools"]
async fn vp9_and_opus_are_re_encoded_for_mov() {
    let folder = tempfile::tempdir().unwrap();
    let input = clip(folder.path(), "clip.mkv", &["libvpx-vp9"], "libopus");
    let (output, notes) = convert(&input, VideoContainer::Mov).await;
    assert_eq!(codecs(&output).0, ["h264", "aac", "mov_text"]);
    assert!(notes[0].contains("vp9 video or opus audio"), "{notes:?}");
}

#[tokio::test]
#[ignore = "needs the installed pinned tools"]
async fn anything_becomes_xvid_and_mp3_in_avi() {
    let folder = tempfile::tempdir().unwrap();
    let input = clip(folder.path(), "clip.webm", &["libvpx-vp9"], "libopus");
    let (output, _) = convert(&input, VideoContainer::Avi).await;
    assert_eq!(codecs(&output).0, ["mpeg4", "mp3"]);
}

#[tokio::test]
#[ignore = "needs the installed pinned tools"]
async fn h264_becomes_vp9_in_webm_while_opus_is_copied() {
    let folder = tempfile::tempdir().unwrap();
    let input = clip(folder.path(), "clip.mkv", &["libx264"], "libopus");
    let (output, notes) = convert(&input, VideoContainer::Webm).await;
    assert_eq!(codecs(&output).0, ["vp9", "opus", "webvtt"]);
    assert!(notes[0].contains("h264 video, so it is"), "{notes:?}");
}
