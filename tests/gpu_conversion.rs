//! Converts a short AV1 clip on the GPU with hardware decoding requested.
//! Needs an NVENC or AMF GPU and the installed tools, so it only runs on
//! request:
//!
//! ```text
//! YT_DLP_TUI_LIVE_TOOLS="$(cargo run -q -- paths | sed -n 's/^Tools: //p')" \
//!     cargo test --test gpu_conversion -- --ignored --nocapture
//! ```
//!
//! GPUs older than NVIDIA Ampere or AMD RDNA 2 can't decode AV1, so there it
//! checks the CPU-decode fallback; newer ones decode on the GPU.

use std::{path::PathBuf, process::Command, time::Duration};

use tokio_util::sync::CancellationToken;
use yt_dlp_tui::{
    domain::{HardwareCodec, VideoConversionOptions},
    integration::ffmpeg::{inspect_hardware_acceleration, run_hardware_conversion},
};

fn ffmpeg() -> PathBuf {
    PathBuf::from(
        std::env::var_os("YT_DLP_TUI_LIVE_TOOLS")
            .expect("set YT_DLP_TUI_LIVE_TOOLS to the installed tools folder"),
    )
    .join(if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    })
}

#[tokio::test]
#[ignore = "needs an NVENC or AMF GPU and the installed pinned tools"]
async fn converts_av1_whether_or_not_the_gpu_can_decode_it() {
    let ffmpeg = ffmpeg();
    let hardware = inspect_hardware_acceleration(Some(&ffmpeg)).await;
    let encoder = hardware
        .encoder_for(&HardwareCodec::H264)
        .expect("an H.264 GPU encoder is available")
        .clone();
    assert!(
        encoder.decode_available,
        "the startup check found a GPU decoder"
    );

    let folder = tempfile::tempdir().unwrap();
    let input = folder.path().join("clip.webm");
    let made = Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i"])
        .arg("testsrc2=size=640x360:rate=30:duration=2")
        .args(["-c:v", "libsvtav1", "-preset", "12"])
        .arg(&input)
        .status()
        .unwrap();
    assert!(made.success(), "FFmpeg made the AV1 test clip");

    let (events, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let options = VideoConversionOptions {
        codec: HardwareCodec::H264,
        quality: 30,
        use_hardware_decode: true,
    };
    let outcome = tokio::time::timeout(
        Duration::from_secs(60),
        run_hardware_conversion(
            &ffmpeg,
            &input,
            &options,
            &encoder,
            CancellationToken::new(),
            events,
        ),
    )
    .await
    .expect("the conversion finishes instead of failing on every frame")
    .expect("the conversion succeeds");

    for line in &outcome.diagnostics {
        println!("{line}");
    }
    assert!(!outcome.cancelled);
    assert!(outcome.output_path.is_file(), "the converted file exists");
    assert!(!input.exists(), "the source is replaced by the conversion");
}
