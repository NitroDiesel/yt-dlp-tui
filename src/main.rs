use std::{
    fs::File,
    io::{IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, atomic::AtomicBool},
};

use yt_dlp_tui::{
    application::AppService,
    integration::installer::{self, InstallProgress},
    tui,
};

const USAGE: &str = "\
yt-dlp TUI: a keyboard-driven terminal interface for yt-dlp

Usage:
  yt-dlp-tui [LINK]                  Open the interface, optionally with a link to download
  yt-dlp-tui install-tools [--reinstall]
                                     Download the pinned, checksum-verified yt-dlp,
                                     FFmpeg, and Deno builds
  yt-dlp-tui paths                   Show where downloads data and tools are kept
  yt-dlp-tui --version               Print the version and pinned tool versions
  yt-dlp-tui --help                  Show this help

Environment:
  YT_DLP_TUI_DATA_DIR    Folder for the downloads database (default: the platform data folder)
  YT_DLP_TUI_TOOLS_DIR   Folder for the pinned tools (default: <data folder>/tools)
  NO_COLOR               Turn colors off
";

fn data_dir() -> PathBuf {
    std::env::var_os("YT_DLP_TUI_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|path| path.join("yt-dlp-tui")))
        .unwrap_or_else(|| PathBuf::from(".yt-dlp-tui"))
}

fn tools_dir() -> PathBuf {
    std::env::var_os("YT_DLP_TUI_TOOLS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir().join("tools"))
}

fn pinned_versions() -> String {
    installer::manifest()
        .components
        .iter()
        .map(|component| format!("{} {}", component.name, component.version))
        .collect::<Vec<_>>()
        .join(", ")
}

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["-h" | "--help"] => {
            print!("{USAGE}");
            Ok(())
        }
        ["-V" | "--version"] => {
            println!("yt-dlp-tui {}", env!("CARGO_PKG_VERSION"));
            println!("Pinned tools: {}", pinned_versions());
            Ok(())
        }
        ["paths"] => {
            println!("Data:  {}", data_dir().display());
            println!("Tools: {}", tools_dir().display());
            Ok(())
        }
        ["install-tools"] => install_tools(false),
        ["install-tools", "--reinstall"] => install_tools(true),
        [] => run_interface(None),
        [link] if !link.starts_with('-') => run_interface(Some(link.to_owned())),
        _ => Err(format!("Unrecognized arguments.\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn install_tools(reinstall: bool) -> Result<(), String> {
    let directory = tools_dir();
    println!(
        "Installing {} into {}",
        pinned_versions(),
        directory.display()
    );
    let mut last = String::new();
    let installed = installer::install(&directory, reinstall, &AtomicBool::new(false), |event| {
        let line = match event {
            InstallProgress::Downloading {
                component,
                downloaded,
                total: Some(total),
            } => format!("{component}: {}%", downloaded * 100 / total.max(1)),
            InstallProgress::Downloading { component, .. } => format!("{component}: downloading"),
            InstallProgress::Extracting { component } => {
                format!("{component}: verified, installing")
            }
            InstallProgress::Installed { component, version } => {
                format!("{component} {version}: installed\n")
            }
        };
        if line != last {
            print!("\r\x1b[2K{line}");
            let _ = std::io::stdout().flush();
            last = line;
        }
    })
    .map_err(|error| error.to_string())?;
    if installed.is_empty() {
        println!("The pinned tools are already installed. Use --reinstall to replace them.");
    }
    Ok(())
}

/// Holds an exclusive lock on the data folder while the interface runs, so a
/// second copy can't schedule the same queued downloads.
fn lock_data_dir(directory: &std::path::Path) -> Result<File, String> {
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("Can't create {}: {error}", directory.display()))?;
    let lock = File::create(directory.join("instance.lock"))
        .map_err(|error| format!("Can't open the instance lock: {error}"))?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(std::fs::TryLockError::WouldBlock) => Err(
            "yt-dlp TUI is already running in another terminal. Use that window, or quit it first."
                .into(),
        ),
        Err(std::fs::TryLockError::Error(error)) => {
            Err(format!("Can't lock {}: {error}", directory.display()))
        }
    }
}

fn run_interface(link: Option<String>) -> Result<(), String> {
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return Err(
            "yt-dlp-tui needs an interactive terminal. See --help for other commands.".into(),
        );
    }
    let data = data_dir();
    let _lock = lock_data_dir(&data)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async move {
        let (events_tx, events) = tokio::sync::mpsc::unbounded_channel();
        let service = AppService::new(data, tools_dir(), events_tx)
            .await
            .map_err(|error| error.to_string())?;
        service.start().await.map_err(|error| error.to_string())?;
        tui::run(Arc::clone(&service), events, link)
            .await
            .map_err(|error| error.to_string())
    })
}
