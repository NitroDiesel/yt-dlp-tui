//! Converts a finished download into WebM, MOV, or AVI. Each accepts only some
//! codecs, so FFprobe lists the streams first: streams that fit are copied and
//! the rest are re-encoded, which keeps the common case a fast lossless remux.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use serde::Deserialize;
use tokio::{process::Command, time::timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    domain::VideoContainer,
    error::{AppError, AppResult},
    integration::{
        ffmpeg::{ConversionOutcome, run_ffmpeg},
        process::configure_background_process,
        yt_dlp::RunnerEvent,
    },
};

const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
    Other,
}

/// One stream of the downloaded file, as FFprobe reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stream {
    pub index: usize,
    pub kind: StreamKind,
    pub codec: String,
    /// An embedded thumbnail, which FFprobe lists as a video stream.
    pub cover: bool,
}

/// An encoder and its options, applied to one output stream.
type Encoder = (&'static str, &'static [(&'static str, &'static str)]);

const H264: Encoder = (
    "libx264",
    &[("preset", "fast"), ("crf", "20"), ("pix_fmt", "yuv420p")],
);
const VP9: Encoder = (
    "libvpx-vp9",
    &[
        ("crf", "32"),
        ("b", "0"),
        ("row-mt", "1"),
        ("deadline", "good"),
        ("cpu-used", "4"),
    ],
);
// Xvid and MP3 are what older players expect inside AVI.
const XVID: Encoder = (
    "libxvid",
    &[("q", "3"), ("pix_fmt", "yuv420p"), ("tag", "XVID")],
);
const AAC: Encoder = ("aac", &[("b", "192k")]);
const OPUS: Encoder = ("libopus", &[("b", "160k")]);
const MP3: Encoder = ("libmp3lame", &[("b", "192k")]);
const MOV_TEXT: Encoder = ("mov_text", &[]);
const WEBVTT: Encoder = ("webvtt", &[]);

const TEXT_SUBTITLES: [&str; 6] = ["subrip", "srt", "ass", "ssa", "webvtt", "mov_text"];

enum Handling {
    Copy,
    Encode(Encoder),
    Drop,
}

fn handling(container: VideoContainer, stream: &Stream) -> Handling {
    use Handling::{Copy, Drop, Encode};
    use StreamKind::{Audio, Subtitle, Video};
    use VideoContainer::{Avi, Mkv, Mov, Mp4, Webm};
    let codec = stream.codec.as_str();
    let text = TEXT_SUBTITLES.contains(&codec);
    match (container, stream.kind) {
        (Mp4 | Mkv, Video | Audio | Subtitle) => Copy,
        (Mov, Video) if stream.cover => match codec {
            "mjpeg" | "png" => Copy,
            _ => Drop,
        },
        (Webm | Avi, Video) if stream.cover => Drop,
        (Mov, Video) => match codec {
            "h264" | "hevc" | "mpeg4" | "prores" => Copy,
            _ => Encode(H264),
        },
        (Mov, Audio) => match codec {
            "aac" | "mp3" | "alac" => Copy,
            codec if codec.starts_with("pcm_") => Copy,
            _ => Encode(AAC),
        },
        (Mov, Subtitle) if codec == "mov_text" => Copy,
        (Mov, Subtitle) if text => Encode(MOV_TEXT),
        (Webm, Video) => match codec {
            "vp8" | "vp9" | "av1" => Copy,
            _ => Encode(VP9),
        },
        (Webm, Audio) => match codec {
            "opus" | "vorbis" => Copy,
            _ => Encode(OPUS),
        },
        (Webm, Subtitle) if codec == "webvtt" => Copy,
        (Webm, Subtitle) if text => Encode(WEBVTT),
        (Avi, Video) if codec == "mpeg4" => Copy,
        (Avi, Video) => Encode(XVID),
        (Avi, Audio) if codec == "mp3" => Copy,
        (Avi, Audio) => Encode(MP3),
        // AVI has no subtitles; image subtitles and attachments fit nowhere here.
        _ => Drop,
    }
}

/// The FFmpeg arguments that map and encode `streams` for `container`, and a
/// note naming each stream that has to be re-encoded.
pub fn conversion_args(
    container: VideoContainer,
    streams: &[Stream],
) -> AppResult<(Vec<String>, Option<String>)> {
    let mut args = Vec::new();
    let mut reencoded = Vec::new();
    let mut output = 0;
    for stream in streams {
        let encoder = match handling(container, stream) {
            Handling::Drop => continue,
            Handling::Copy => None,
            Handling::Encode(encoder) => Some(encoder),
        };
        args.extend(["-map".into(), format!("0:{}", stream.index)]);
        match encoder {
            None => {
                args.extend([format!("-c:{output}"), "copy".into()]);
                if container == VideoContainer::Mov && stream.codec == "hevc" {
                    // QuickTime only plays HEVC tagged this way.
                    args.extend([format!("-tag:{output}"), "hvc1".into()]);
                }
            }
            Some((name, options)) => {
                args.extend([format!("-c:{output}"), name.into()]);
                for (option, value) in options {
                    args.extend([format!("-{option}:{output}"), (*value).into()]);
                }
                if stream.kind != StreamKind::Subtitle {
                    reencoded.push(format!("{} {}", stream.codec, kind_name(stream.kind)));
                }
            }
        }
        output += 1;
    }
    if !streams
        .iter()
        .any(|stream| matches!(stream.kind, StreamKind::Video | StreamKind::Audio) && !stream.cover)
    {
        return Err(AppError::Conversion(format!(
            "The download has no video or audio to put in a {} file",
            container.label()
        )));
    }
    let note = (!reencoded.is_empty()).then(|| {
        format!(
            "{} can't hold the downloaded {}, so {} re-encoded. Everything else is copied as is.",
            container.label(),
            reencoded.join(" or "),
            if reencoded.len() == 1 {
                "it is"
            } else {
                "they are"
            }
        )
    });
    Ok((args, note))
}

fn kind_name(kind: StreamKind) -> &'static str {
    match kind {
        StreamKind::Video => "video",
        StreamKind::Audio => "audio",
        StreamKind::Subtitle => "subtitles",
        StreamKind::Other => "data",
    }
}

#[derive(Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Deserialize)]
struct ProbeStream {
    index: usize,
    codec_type: Option<String>,
    codec_name: Option<String>,
    disposition: Option<ProbeDisposition>,
}

#[derive(Deserialize)]
struct ProbeDisposition {
    #[serde(default)]
    attached_pic: u8,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

/// Parses `ffprobe -of json` output into streams and the duration in seconds.
pub fn parse_probe(json: &[u8]) -> AppResult<(Vec<Stream>, Option<f64>)> {
    let output: ProbeOutput = serde_json::from_slice(json)
        .map_err(|error| AppError::Conversion(format!("FFprobe output was unreadable: {error}")))?;
    let streams = output
        .streams
        .into_iter()
        .map(|stream| Stream {
            index: stream.index,
            kind: match stream.codec_type.as_deref() {
                Some("video") => StreamKind::Video,
                Some("audio") => StreamKind::Audio,
                Some("subtitle") => StreamKind::Subtitle,
                _ => StreamKind::Other,
            },
            codec: stream.codec_name.unwrap_or_default(),
            cover: stream
                .disposition
                .is_some_and(|disposition| disposition.attached_pic == 1),
        })
        .collect();
    let duration = output
        .format
        .and_then(|format| format.duration)
        .and_then(|duration| duration.parse().ok());
    Ok((streams, duration))
}

async fn probe(ffprobe: &Path, input: &Path) -> AppResult<(Vec<Stream>, Option<f64>)> {
    let mut command = Command::new(ffprobe);
    command
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=index,codec_type,codec_name:stream_disposition=attached_pic:format=duration",
            "-of",
            "json",
            "-i",
        ])
        .arg(input)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    configure_background_process(&mut command);
    let output = match timeout(PROBE_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            return Err(AppError::Conversion(format!(
                "FFprobe could not be started: {error}"
            )));
        }
        Err(_) => {
            return Err(AppError::Conversion(
                "FFprobe took too long to read the download".into(),
            ));
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Conversion(format!(
            "FFprobe could not read the download: {}",
            stderr.trim().chars().take(240).collect::<String>()
        )));
    }
    parse_probe(&output.stdout)
}

/// Whether `path` already has the container's extension, so it needs no work.
pub fn already_in(path: &Path, container: VideoContainer) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(container.as_str()))
}

/// Converts `input` into `container` beside it, then deletes `input`. The
/// original is kept when the conversion fails or is cancelled.
pub async fn run_container_conversion(
    ffmpeg: &Path,
    ffprobe: &Path,
    input: &Path,
    container: VideoContainer,
    cancel: CancellationToken,
    events: tokio::sync::mpsc::UnboundedSender<RunnerEvent>,
) -> AppResult<ConversionOutcome> {
    let input = tokio::fs::canonicalize(input).await?;
    if !tokio::fs::metadata(&input).await?.is_file() {
        return Err(AppError::Validation(
            "The downloaded source is not a regular file".into(),
        ));
    }
    let (streams, duration) = probe(ffprobe, &input).await?;
    let (stream_args, note) = conversion_args(container, &streams)?;
    let output_path = available_output_path(&input, container)?;
    let temporary_path = output_path.with_file_name(format!(
        ".{}.{}.part.{}",
        output_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("conversion"),
        Uuid::new_v4(),
        container.as_str()
    ));
    let mut command = Command::new(ffmpeg);
    command
        .args([
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "warning",
            "-y",
            "-i",
        ])
        .arg(&input)
        .args(stream_args)
        .args(["-map_metadata", "0", "-map_chapters", "0"]);
    if container == VideoContainer::Mov {
        command.args(["-movflags", "+faststart"]);
    }
    command
        .args([
            "-max_muxing_queue_size",
            "4096",
            "-progress",
            "pipe:1",
            "-nostats",
            "-f",
            match container {
                VideoContainer::Mkv => "matroska",
                other => other.as_str(),
            },
        ])
        .arg(&temporary_path);
    if let Some(note) = &note {
        let _ = events.send(RunnerEvent::Diagnostic(note.clone()));
    }
    let (status, diagnostics) = run_ffmpeg(
        command,
        format!("Converting to {}", container.label()),
        duration,
        note.into_iter().collect(),
        cancel,
        events,
    )
    .await?;
    match status {
        None => {
            let _ = tokio::fs::remove_file(&temporary_path).await;
            Ok(ConversionOutcome {
                output_path: input,
                diagnostics,
                cancelled: true,
            })
        }
        Some(status) if status.success() => {
            let current_input = tokio::fs::canonicalize(&input).await?;
            if current_input != input || !tokio::fs::metadata(&current_input).await?.is_file() {
                let _ = tokio::fs::remove_file(&temporary_path).await;
                return Err(AppError::Validation(
                    "The downloaded source changed while it was being converted".into(),
                ));
            }
            tokio::fs::rename(&temporary_path, &output_path).await?;
            tokio::fs::remove_file(&input).await?;
            Ok(ConversionOutcome {
                output_path,
                diagnostics,
                cancelled: false,
            })
        }
        Some(status) => {
            let _ = tokio::fs::remove_file(&temporary_path).await;
            Err(AppError::Conversion(format!(
                "FFmpeg stopped with {status} while converting to {}. The original download was kept; the technical details show why.",
                container.label()
            )))
        }
    }
}

/// `name.mov` beside the input, or `name (2).mov` and so on when taken.
fn available_output_path(input: &Path, container: VideoContainer) -> AppResult<PathBuf> {
    let parent = input
        .parent()
        .ok_or_else(|| AppError::Validation("The download has no parent folder".into()))?;
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("download");
    for index in 1..=100 {
        let name = if index == 1 {
            format!("{stem}.{}", container.as_str())
        } else {
            format!("{stem} ({index}).{}", container.as_str())
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(AppError::Validation(format!(
        "Could not choose a unique name for the {} file",
        container.label()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(index: usize, kind: StreamKind, codec: &str) -> Stream {
        Stream {
            index,
            kind,
            codec: codec.into(),
            cover: false,
        }
    }

    fn pairs(args: &[String]) -> Vec<(String, String)> {
        args.chunks(2)
            .map(|pair| (pair[0].clone(), pair[1].clone()))
            .collect()
    }

    fn has(args: &[String], flag: &str, value: &str) -> bool {
        pairs(args)
            .iter()
            .any(|(left, right)| left == flag && right == value)
    }

    #[test]
    fn copies_streams_mov_can_hold() {
        let streams = [
            stream(0, StreamKind::Video, "h264"),
            stream(1, StreamKind::Audio, "aac"),
        ];
        let (args, note) = conversion_args(VideoContainer::Mov, &streams).unwrap();
        assert_eq!(
            args,
            ["-map", "0:0", "-c:0", "copy", "-map", "0:1", "-c:1", "copy"]
        );
        assert_eq!(note, None, "nothing is re-encoded");
    }

    #[test]
    fn re_encodes_only_the_streams_that_do_not_fit() {
        let streams = [
            stream(0, StreamKind::Video, "h264"),
            stream(1, StreamKind::Audio, "opus"),
        ];
        let (args, note) = conversion_args(VideoContainer::Mov, &streams).unwrap();
        assert!(has(&args, "-c:0", "copy"));
        assert!(has(&args, "-c:1", "aac"));
        assert!(has(&args, "-b:1", "192k"));
        assert_eq!(
            note.as_deref(),
            Some(
                "MOV can't hold the downloaded opus audio, so it is re-encoded. Everything else is copied as is."
            )
        );

        let streams = [
            stream(0, StreamKind::Video, "vp9"),
            stream(1, StreamKind::Audio, "opus"),
        ];
        let (args, _) = conversion_args(VideoContainer::Mov, &streams).unwrap();
        assert!(has(&args, "-c:0", "libx264"));
        assert!(has(&args, "-pix_fmt:0", "yuv420p"));
    }

    #[test]
    fn tags_hevc_so_quicktime_plays_it() {
        let streams = [stream(0, StreamKind::Video, "hevc")];
        let (args, _) = conversion_args(VideoContainer::Mov, &streams).unwrap();
        assert!(has(&args, "-c:0", "copy"));
        assert!(has(&args, "-tag:0", "hvc1"));
    }

    #[test]
    fn webm_keeps_vp9_av1_and_opus_and_re_encodes_the_rest() {
        let streams = [
            stream(0, StreamKind::Video, "av1"),
            stream(1, StreamKind::Audio, "opus"),
        ];
        let (args, note) = conversion_args(VideoContainer::Webm, &streams).unwrap();
        assert!(has(&args, "-c:0", "copy") && has(&args, "-c:1", "copy"));
        assert_eq!(note, None);

        let streams = [
            stream(0, StreamKind::Video, "h264"),
            stream(1, StreamKind::Audio, "aac"),
        ];
        let (args, _) = conversion_args(VideoContainer::Webm, &streams).unwrap();
        assert!(has(&args, "-c:0", "libvpx-vp9"));
        assert!(has(&args, "-c:1", "libopus"));
    }

    #[test]
    fn avi_becomes_xvid_and_mp3() {
        let streams = [
            stream(0, StreamKind::Video, "h264"),
            stream(1, StreamKind::Audio, "aac"),
            stream(2, StreamKind::Subtitle, "subrip"),
        ];
        let (args, note) = conversion_args(VideoContainer::Avi, &streams).unwrap();
        assert!(has(&args, "-c:0", "libxvid"));
        assert!(has(&args, "-tag:0", "XVID"));
        assert!(has(&args, "-c:1", "libmp3lame"));
        assert!(!args.contains(&"0:2".to_string()), "AVI has no subtitles");
        assert!(note.unwrap().contains("they are re-encoded"));
    }

    #[test]
    fn converts_text_subtitles_and_drops_what_cannot_fit() {
        let mut cover = stream(1, StreamKind::Video, "mjpeg");
        cover.cover = true;
        let streams = [
            stream(0, StreamKind::Video, "vp9"),
            cover,
            stream(2, StreamKind::Audio, "opus"),
            stream(3, StreamKind::Subtitle, "ass"),
            stream(4, StreamKind::Subtitle, "hdmv_pgs_subtitle"),
            stream(5, StreamKind::Other, "ttf"),
        ];
        let (args, _) = conversion_args(VideoContainer::Webm, &streams).unwrap();
        // Output streams are numbered after drops: video, audio, subtitles.
        assert_eq!(
            pairs(&args)
                .iter()
                .filter(|(flag, _)| flag == "-map")
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>(),
            ["0:0", "0:2", "0:3"]
        );
        assert!(has(&args, "-c:2", "webvtt"));

        let (args, _) = conversion_args(VideoContainer::Mov, &streams).unwrap();
        assert!(has(&args, "-map", "0:1"), "MOV keeps the cover");
        assert!(has(&args, "-c:3", "mov_text"));
    }

    #[test]
    fn refuses_files_with_nothing_to_convert() {
        let streams = [stream(0, StreamKind::Subtitle, "webvtt")];
        assert!(conversion_args(VideoContainer::Mov, &streams).is_err());
    }

    #[test]
    fn reads_ffprobe_json() {
        let json = br#"{"programs":[],"streams":[
            {"index":0,"codec_name":"vp9","codec_type":"video","disposition":{"attached_pic":0}},
            {"index":1,"codec_name":"mjpeg","codec_type":"video","disposition":{"attached_pic":1}},
            {"index":2,"codec_name":"opus","codec_type":"audio","disposition":{"attached_pic":0}},
            {"index":3,"codec_type":"attachment"}],
            "format":{"duration":"12.500000"}}"#;
        let (streams, duration) = parse_probe(json).unwrap();
        assert_eq!(duration, Some(12.5));
        assert_eq!(streams.len(), 4);
        assert!(streams[1].cover && streams[1].kind == StreamKind::Video);
        assert_eq!(streams[2].codec, "opus");
        assert_eq!(streams[3].kind, StreamKind::Other);
    }

    #[test]
    fn names_the_output_beside_the_input_without_overwriting() {
        let folder = tempfile::tempdir().unwrap();
        let input = folder.path().join("video.webm");
        assert_eq!(
            available_output_path(&input, VideoContainer::Mov).unwrap(),
            folder.path().join("video.mov")
        );
        std::fs::write(folder.path().join("video.mov"), b"taken").unwrap();
        assert_eq!(
            available_output_path(&input, VideoContainer::Mov).unwrap(),
            folder.path().join("video (2).mov")
        );
        assert!(already_in(Path::new("/x/clip.MOV"), VideoContainer::Mov));
        assert!(!already_in(Path::new("/x/clip.webm"), VideoContainer::Mov));
    }
}
