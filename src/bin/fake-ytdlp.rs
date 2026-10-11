//! A stand-in for yt-dlp used by tests. The link decides the scenario:
//! links containing "cancel" never finish, "failure" fails like a rate limit,
//! "refused" analyzes but fails to download with warnings first, "slow"
//! reports progress for a few seconds, "playlist" analyzes as a three-item
//! playlist, and anything else succeeds.

use std::{path::PathBuf, thread, time::Duration};

fn progress(downloaded: u64, total: u64, speed: u64, eta: u64) {
    println!(
        r#"__YTDLP_TUI__{{"v":1,"kind":"download","status":"downloading","downloadedBytes":{downloaded},"totalBytes":{total},"totalBytesEstimate":null,"speed":{speed},"eta":{eta},"filename":"fixture.part","playlistIndex":null,"playlistCount":null}}"#
    );
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--version") {
        println!("2026.08.19");
        return;
    }
    let scenario = args.last().map(String::as_str).unwrap_or_default();
    if args.iter().any(|arg| arg == "--dump-single-json") {
        if scenario.contains("failure") {
            eprintln!("ERROR: [generic] Unsupported URL: {scenario}");
            std::process::exit(1);
        }
        if scenario.contains("playlist") {
            println!(
                r#"{{"id":"list","title":"Fixture playlist","uploader":"Fixture channel","webpage_url":"{scenario}","n_entries":3,"entries":[{{"id":"a"}},{{"id":"b"}},{{"id":"c"}}]}}"#
            );
            return;
        }
        println!(
            r#"{{"id":"fixture","title":"Fixture media","uploader":"Fixture channel","webpage_url":"{scenario}","duration":42,"formats":[{{"format_id":"137","ext":"mp4","height":1080,"fps":30,"vcodec":"avc1.640028","acodec":"none","tbr":4000,"filesize":21000000}},{{"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a.40.2","tbr":128,"language":"en"}}],"subtitles":{{"en":[{{"ext":"vtt"}}]}}}}"#
        );
        return;
    }
    if scenario.contains("cancel") {
        loop {
            thread::sleep(Duration::from_millis(100));
        }
    }
    if scenario.contains("failure") {
        eprintln!("ERROR: HTTP Error 429: Too Many Requests token=private-value");
        std::process::exit(1);
    }
    if scenario.contains("refused") {
        eprintln!("WARNING: [fixture] The first format was refused, trying the next");
        eprintln!("ERROR: unable to download video data: HTTP Error 403: Forbidden");
        std::process::exit(1);
    }
    if scenario.contains("slow") {
        for step in 1..=30 {
            progress(step * 700_000, 21_000_000, 7_000_000, 30 - step);
            thread::sleep(Duration::from_millis(100));
        }
    } else {
        progress(25, 100, 50, 2);
    }
    // Like yt-dlp in quiet mode, postprocessor records go to stderr.
    eprintln!(
        r#"__YTDLP_TUI__{{"v":1,"kind":"postprocess","status":"started","postprocessor":"Merger","filename":"fixture.webm"}}"#
    );
    // With a destination (`-P`), write a real file there so the app can verify it.
    let destination = args
        .windows(2)
        .find(|pair| pair[0] == "-P")
        .map(|pair| PathBuf::from(&pair[1]));
    let output = match destination {
        Some(folder) => {
            let path = folder.join("Fixture media.webm");
            std::fs::write(&path, b"fixture media").expect("destination is writable");
            path.to_string_lossy().into_owned()
        }
        None => "fixture.webm".into(),
    };
    println!(
        r#"__YTDLP_TUI__{{"v":1,"kind":"after_move","filepath":{},"title":"Fixture media"}}"#,
        serde_json::to_string(&output).expect("a path serializes")
    );
}
