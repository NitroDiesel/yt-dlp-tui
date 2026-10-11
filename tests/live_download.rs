//! Downloads a short public video with the real pinned tools.
//! Needs the network and installed tools, so it only runs on request:
//!
//! ```text
//! cargo run -- install-tools
//! YT_DLP_TUI_LIVE_TOOLS="$(cargo run -q -- paths | sed -n 's/^Tools: //p')" \
//!     cargo test --test live_download -- --ignored --nocapture
//! ```

use std::{path::PathBuf, time::Duration};

use yt_dlp_tui::{
    application::{AppService, ServiceEvent},
    domain::{
        AudioFormat, ClipOptions, DownloadJob, DownloadOptions, DownloadRequest, JobStatus,
        MediaMode, SponsorBlockMode, SponsorBlockOptions, SponsorCategory, VideoContainer,
    },
};

/// "Me at the zoo": 19 seconds, public, and online since 2005.
const TEST_VIDEO: &str = "https://www.youtube.com/watch?v=jNQXAC9IVRw";

fn tools() -> PathBuf {
    PathBuf::from(
        std::env::var_os("YT_DLP_TUI_LIVE_TOOLS")
            .expect("set YT_DLP_TUI_LIVE_TOOLS to the installed tools folder"),
    )
}

/// The finished job, the post-processing stages seen while it ran, and
/// whether live byte progress arrived.
async fn finish(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<ServiceEvent>,
    id: &str,
) -> (DownloadJob, Vec<String>, bool) {
    tokio::time::timeout(Duration::from_secs(300), async {
        let mut progress = false;
        let mut stages = Vec::new();
        loop {
            let ServiceEvent::JobChanged(job) = events.recv().await.expect("service stays open");
            if job.id != id {
                continue;
            }
            progress |=
                job.status == JobStatus::Downloading && job.progress.downloaded_bytes.is_some();
            if job.status == JobStatus::PostProcessing
                && let Some(stage) = job.progress.stage.clone()
                && !stages.contains(&stage)
            {
                stages.push(stage);
            }
            if matches!(
                job.status,
                JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
            ) {
                return (*job, stages, progress);
            }
        }
    })
    .await
    .expect("the download finishes within five minutes")
}

#[tokio::test]
#[ignore = "downloads from YouTube with the installed pinned tools"]
async fn analyzes_and_downloads_with_the_pinned_engine() {
    let data = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let service = AppService::new(data.path().to_path_buf(), tools(), tx)
        .await
        .unwrap();

    let dependencies = service.dependencies().await;
    for dependency in &dependencies {
        println!(
            "{:?}: {} {}",
            dependency.kind,
            dependency.status,
            dependency.version.as_deref().unwrap_or("")
        );
        assert_eq!(dependency.status, "available", "{:?}", dependency.kind);
    }

    let probe = service.probe_media(TEST_VIDEO.into(), false).await.unwrap();
    println!(
        "Analyzed \"{}\" by {:?}: {} formats, {} subtitle tracks",
        probe.title,
        probe.creator,
        probe.formats.len(),
        probe.subtitles.len()
    );
    assert!(!probe.is_playlist);
    assert!(!probe.formats.is_empty());

    let request = |options: DownloadOptions| DownloadRequest {
        url: probe.url.clone(),
        destination: destination.path().to_string_lossy().into_owned(),
        filename_template: "%(title).80B [%(id)s].%(ext)s".into(),
        is_playlist: false,
        options,
    };

    // Merged best video with SponsorBlock chapters and a precise clip.
    let video = service
        .enqueue(
            request(DownloadOptions {
                quality: "720".into(),
                embed_chapters: true,
                sponsorblock: Some(SponsorBlockOptions {
                    mode: SponsorBlockMode::Mark,
                    categories: vec![SponsorCategory::Sponsor, SponsorCategory::Hook],
                }),
                clip: Some(ClipOptions {
                    start_seconds: 1.0,
                    duration_seconds: 4.0,
                    precise: true,
                }),
                ..DownloadOptions::default()
            }),
            true,
            Some(probe.title.clone()),
        )
        .await
        .unwrap();
    let (video, stages, _) = finish(&mut events, &video.id).await;
    println!(
        "Video job: {:?} {:?} after {stages:?}",
        video.status, video.output_path
    );
    if video.status != JobStatus::Completed {
        println!("{}", video.diagnostics.join("\n"));
    }
    assert_eq!(
        video.status,
        JobStatus::Completed,
        "{:?}",
        video.error_message
    );
    let output = PathBuf::from(video.output_path.unwrap());
    assert!(output.starts_with(destination.path().canonicalize().unwrap()));
    assert!(output.metadata().unwrap().len() > 0);
    assert!(
        output.to_string_lossy().contains("(clip 1-5s)"),
        "{output:?}"
    );

    // Audio extracted and converted to MP3 by FFmpeg.
    let audio = service
        .enqueue(
            request(DownloadOptions {
                mode: MediaMode::Audio,
                audio_format: AudioFormat::Mp3,
                ..DownloadOptions::default()
            }),
            true,
            Some(probe.title.clone()),
        )
        .await
        .unwrap();
    let (audio, stages, progress) = finish(&mut events, &audio.id).await;
    assert!(progress, "live download progress reached the app");
    println!(
        "Audio job: {:?} {:?} after {stages:?}",
        audio.status, audio.output_path
    );
    assert!(
        stages.iter().any(|stage| stage == "ExtractAudio"),
        "{stages:?}"
    );
    assert_eq!(
        audio.status,
        JobStatus::Completed,
        "{:?}",
        audio.error_message
    );
    assert!(audio.output_path.unwrap().ends_with(".mp3"));

    // MOV takes YouTube's H.264 and AAC as they are; AVI re-encodes them.
    for container in [VideoContainer::Mov, VideoContainer::Avi] {
        let job = service
            .enqueue(
                request(DownloadOptions {
                    quality: "720".into(),
                    container: Some(container),
                    ..DownloadOptions::default()
                }),
                true,
                Some(probe.title.clone()),
            )
            .await
            .unwrap();
        let (job, stages, _) = finish(&mut events, &job.id).await;
        println!(
            "{} job: {:?} {:?} after {stages:?}",
            container.label(),
            job.status,
            job.output_path
        );
        assert_eq!(
            job.status,
            JobStatus::Completed,
            "{:?}\n{}",
            job.error_message,
            job.diagnostics.join("\n")
        );
        let converting = format!("Converting to {}", container.label());
        assert!(stages.contains(&converting), "{stages:?}");
        let output = PathBuf::from(job.output_path.unwrap());
        assert_eq!(
            output.extension().unwrap(),
            container.as_str(),
            "{output:?}"
        );
        assert!(output.metadata().unwrap().len() > 0);
    }
    // Each conversion replaced its download; only the clip and the MP3 remain.
    let mut leftovers = std::fs::read_dir(destination.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| !name.contains("(clip") && !name.ends_with(".mp3"))
        .collect::<Vec<_>>();
    leftovers.sort();
    assert_eq!(leftovers.len(), 2, "{leftovers:?}");
    assert!(leftovers[0].ends_with(".avi") && leftovers[1].ends_with(".mov"));
}
