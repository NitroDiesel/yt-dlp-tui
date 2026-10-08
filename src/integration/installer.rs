//! Downloads the pinned tool builds listed in `packaging/components.json`,
//! verifies each archive's SHA-256 before using it, and installs the
//! executables into the managed tools folder. Nothing is executed during
//! installation, and a component is only swapped in once all of its files
//! verified and extracted.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    error::{AppError, AppResult},
    integration::dependencies::executable_name,
};

const MANIFEST_JSON: &str = include_str!("../../packaging/components.json");
const INSTALLED_FILE: &str = "installed.json";
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentManifest {
    pub schema_version: u32,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Component {
    pub id: String,
    pub name: String,
    pub version: String,
    pub license: String,
    pub source: String,
    pub targets: BTreeMap<String, Vec<Artifact>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Artifact {
    pub url: String,
    pub sha256: String,
    /// Exact length in bytes; anything else is rejected while downloading.
    pub size: u64,
    pub archive: ArchiveKind,
    /// Installed tool name (without `.exe`) to its path inside the archive.
    pub entries: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum ArchiveKind {
    #[serde(rename = "binary")]
    Binary,
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar.xz")]
    TarXz,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InstallProgress {
    Downloading {
        component: String,
        downloaded: u64,
        total: Option<u64>,
    },
    Extracting {
        component: String,
    },
    Installed {
        component: String,
        version: String,
    },
}

/// The pinned component list compiled into this build.
pub fn manifest() -> &'static ComponentManifest {
    static MANIFEST: LazyLock<ComponentManifest> = LazyLock::new(|| {
        serde_json::from_str(MANIFEST_JSON).expect("packaging/components.json is valid")
    });
    &MANIFEST
}

/// Release target key for this machine, or `None` when no pinned build exists.
pub fn current_target() -> Option<&'static str> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => Some("x86_64-unknown-linux-gnu"),
        ("aarch64", "linux") => Some("aarch64-unknown-linux-gnu"),
        ("x86_64", "windows") => Some("x86_64-pc-windows-msvc"),
        ("aarch64", "macos") => Some("aarch64-apple-darwin"),
        ("x86_64", "macos") => Some("x86_64-apple-darwin"),
        _ => None,
    }
}

/// Component id to installed version, as recorded by the last installation.
pub fn installed_versions(dir: &Path) -> BTreeMap<String, String> {
    fs::read(dir.join(INSTALLED_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Components that are missing, incomplete, or not at the pinned version.
pub fn outdated_components(dir: &Path) -> Vec<&'static Component> {
    let Some(target) = current_target() else {
        return Vec::new();
    };
    let installed = installed_versions(dir);
    manifest()
        .components
        .iter()
        .filter(|component| {
            let complete = component.targets.get(target).is_some_and(|artifacts| {
                artifacts.iter().all(|artifact| {
                    artifact
                        .entries
                        .keys()
                        .all(|tool| dir.join(executable_name(tool)).is_file())
                })
            });
            !complete || installed.get(&component.id) != Some(&component.version)
        })
        .collect()
}

/// Bytes this machine downloads to install `components`.
pub fn download_size(components: &[&Component]) -> u64 {
    let Some(target) = current_target() else {
        return 0;
    };
    components
        .iter()
        .filter_map(|component| component.targets.get(target))
        .flatten()
        .map(|artifact| artifact.size)
        .sum()
}

/// Installs every component that `outdated_components` reports, or all of
/// them when `reinstall` is set. Returns the names of installed components.
pub fn install(
    dir: &Path,
    reinstall: bool,
    cancel: &AtomicBool,
    mut progress: impl FnMut(InstallProgress),
) -> AppResult<Vec<String>> {
    let target = current_target().ok_or_else(|| {
        AppError::Install(format!(
            "No pinned tool builds exist for {} {}. Choose your own executables in Settings.",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })?;
    let components: Vec<&Component> = if reinstall {
        manifest().components.iter().collect()
    } else {
        outdated_components(dir)
    };
    fs::create_dir_all(dir)?;
    let agent = ureq::Agent::config_builder()
        .user_agent(concat!("yt-dlp-tui/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_recv_body(Some(Duration::from_secs(30 * 60)))
        .build()
        .new_agent();
    let mut installed = Vec::new();
    for component in components {
        let artifacts = component.targets.get(target).ok_or_else(|| {
            AppError::Install(format!("{} has no build for {target}", component.name))
        })?;
        let staging = Staging::new(dir)?;
        for (index, artifact) in artifacts.iter().enumerate() {
            let archive = staging.path.join(format!("download-{index}"));
            download_verified(&agent, artifact, &archive, cancel, |downloaded, total| {
                progress(InstallProgress::Downloading {
                    component: component.name.clone(),
                    downloaded,
                    total,
                })
            })?;
            progress(InstallProgress::Extracting {
                component: component.name.clone(),
            });
            extract(artifact, &archive, &staging.path)?;
        }
        for artifact in artifacts {
            for tool in artifact.entries.keys() {
                let name = executable_name(tool);
                let staged = staging.path.join(name);
                make_executable(&staged)?;
                replace_file(&staged, &dir.join(name))?;
            }
        }
        record_installed(dir, &component.id, &component.version)?;
        progress(InstallProgress::Installed {
            component: component.name.clone(),
            version: component.version.clone(),
        });
        installed.push(component.name.clone());
    }
    Ok(installed)
}

/// A private scratch folder inside the tools folder, removed on drop, so a
/// failed or cancelled installation never leaves partial executables behind.
struct Staging {
    path: PathBuf,
}

impl Staging {
    fn new(dir: &Path) -> AppResult<Self> {
        let path = dir.join(format!(".staging-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn download_verified(
    agent: &ureq::Agent,
    artifact: &Artifact,
    destination: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, Option<u64>),
) -> AppResult<()> {
    let mut response = agent
        .get(&artifact.url)
        .call()
        .map_err(|error| AppError::Install(format!("{}: {error}", artifact.url)))?;
    if let Some(length) = response.body().content_length() {
        check_size(length, artifact)?;
    }
    let total = Some(artifact.size);
    let mut reader = response.body_mut().as_reader();
    let mut file = File::create(destination)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut downloaded = 0_u64;
    let mut last_report: Option<Instant> = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Install("Installation cancelled".into()));
        }
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(AppError::Install(format!("{}: {error}", artifact.url)));
            }
        };
        downloaded += read as u64;
        if downloaded > artifact.size {
            check_size(downloaded, artifact)?;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])?;
        if last_report.is_none_or(|last| last.elapsed() >= PROGRESS_INTERVAL) {
            last_report = Some(Instant::now());
            progress(downloaded, total);
        }
    }
    file.sync_all()?;
    progress(downloaded, total);
    check_size(downloaded, artifact)?;
    verify_digest(&hasher.finalize(), &artifact.sha256, &artifact.url)
}

fn check_size(bytes: u64, artifact: &Artifact) -> AppResult<()> {
    if bytes == artifact.size {
        Ok(())
    } else {
        Err(AppError::Install(format!(
            "{} is {bytes} bytes, but the pinned file is {} bytes. Nothing was installed.",
            artifact.url, artifact.size
        )))
    }
}

fn verify_digest(actual: &[u8], expected: &str, label: &str) -> AppResult<()> {
    let actual = actual
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(AppError::Install(format!(
            "Checksum mismatch for {label}: expected {expected}, got {actual}. Nothing was installed."
        )))
    }
}

/// Copies each listed entry out of the archive into `staging` under its tool
/// name. Only the named entries are read, so archive paths never decide where
/// files are written.
fn extract(artifact: &Artifact, archive: &Path, staging: &Path) -> AppResult<()> {
    match artifact.archive {
        ArchiveKind::Binary => {
            let [(tool, _)] = artifact.entries.iter().collect::<Vec<_>>()[..] else {
                return Err(AppError::Install(
                    "A plain binary download must install exactly one tool".into(),
                ));
            };
            fs::rename(archive, staging.join(executable_name(tool)))?;
        }
        ArchiveKind::Zip => {
            let mut zip = zip::ZipArchive::new(File::open(archive)?)
                .map_err(|error| AppError::Install(format!("Unreadable zip archive: {error}")))?;
            for (tool, entry) in &artifact.entries {
                let mut source = zip.by_name(entry).map_err(|_| {
                    AppError::Install(format!("{entry} is missing from {}", artifact.url))
                })?;
                let mut output = File::create(staging.join(executable_name(tool)))?;
                io::copy(&mut source, &mut output)?;
            }
        }
        ArchiveKind::TarXz => {
            let decoder = liblzma::read::XzDecoder::new(File::open(archive)?);
            let mut tar = tar::Archive::new(decoder);
            let mut remaining = artifact
                .entries
                .iter()
                .map(|(tool, entry)| (PathBuf::from(entry), tool.as_str()))
                .collect::<BTreeMap<_, _>>();
            for entry in tar.entries()? {
                let mut entry = entry?;
                let path = entry.path()?.into_owned();
                if let Some(tool) = remaining.remove(&path) {
                    let mut output = File::create(staging.join(executable_name(tool)))?;
                    io::copy(&mut entry, &mut output)?;
                }
                if remaining.is_empty() {
                    break;
                }
            }
            if let Some(path) = remaining.keys().next() {
                return Err(AppError::Install(format!(
                    "{} is missing from {}",
                    path.display(),
                    artifact.url
                )));
            }
        }
    }
    Ok(())
}

fn make_executable(path: &Path) -> AppResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn replace_file(staged: &Path, destination: &Path) -> AppResult<()> {
    // Windows cannot rename over an existing file.
    if cfg!(windows) && destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(staged, destination)?;
    Ok(())
}

fn record_installed(dir: &Path, id: &str, version: &str) -> AppResult<()> {
    let mut installed = installed_versions(dir);
    installed.insert(id.into(), version.into());
    let temporary = dir.join(format!(".{INSTALLED_FILE}.tmp"));
    fs::write(&temporary, serde_json::to_vec_pretty(&installed)?)?;
    replace_file(&temporary, &dir.join(INSTALLED_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_manifest_pins_every_supported_target() {
        let manifest = manifest();
        assert_eq!(manifest.schema_version, 1);
        let ids = manifest
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["yt-dlp", "deno", "ffmpeg"]);
        for component in &manifest.components {
            for target in [
                "x86_64-unknown-linux-gnu",
                "aarch64-unknown-linux-gnu",
                "x86_64-pc-windows-msvc",
                "aarch64-apple-darwin",
                "x86_64-apple-darwin",
            ] {
                let artifacts = component
                    .targets
                    .get(target)
                    .unwrap_or_else(|| panic!("{} lacks {target}", component.id));
                for artifact in artifacts {
                    assert!(artifact.url.starts_with("https://"));
                    assert!(artifact.size > 1_000_000, "{} has a size", artifact.url);
                    assert_eq!(artifact.sha256.len(), 64);
                    assert!(artifact.sha256.chars().all(|c| c.is_ascii_hexdigit()));
                }
            }
        }
    }

    #[test]
    fn pins_the_latest_audited_yt_dlp() {
        let yt_dlp = &manifest().components[0];
        assert_eq!(yt_dlp.version, "2026.08.19");
    }

    #[test]
    fn only_the_pinned_size_is_accepted() {
        let artifact = Artifact {
            url: "https://example.test/tool".into(),
            sha256: String::new(),
            size: 100,
            archive: ArchiveKind::Binary,
            entries: BTreeMap::new(),
        };
        assert!(check_size(100, &artifact).is_ok());
        assert!(check_size(99, &artifact).is_err());
        assert!(check_size(101, &artifact).is_err());
    }

    #[test]
    fn reports_the_download_size_for_this_machine() {
        let all = manifest().components.iter().collect::<Vec<_>>();
        if current_target().is_some() {
            assert!(download_size(&all) > download_size(&all[..1]));
        }
        assert_eq!(download_size(&[]), 0);
    }

    #[test]
    fn digest_mismatch_is_rejected() {
        let digest = Sha256::digest(b"payload");
        let hex = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert!(verify_digest(&digest, &hex, "fixture").is_ok());
        assert!(verify_digest(&digest, &hex.to_uppercase(), "fixture").is_ok());
        assert!(verify_digest(&digest, &"0".repeat(64), "fixture").is_err());
    }

    #[test]
    fn extracts_only_named_zip_entries_under_tool_names() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("tools.zip");
        {
            let mut writer = zip::ZipWriter::new(File::create(&archive).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("pkg/bin/deno", options).unwrap();
            writer.write_all(b"deno-binary").unwrap();
            writer.start_file("../escape", options).unwrap();
            writer.write_all(b"ignored").unwrap();
            writer.finish().unwrap();
        }
        let staging = directory.path().join("staging");
        fs::create_dir(&staging).unwrap();
        let artifact = Artifact {
            url: "https://example.test/tools.zip".into(),
            sha256: String::new(),
            size: 0,
            archive: ArchiveKind::Zip,
            entries: BTreeMap::from([("deno".into(), "pkg/bin/deno".into())]),
        };

        extract(&artifact, &archive, &staging).unwrap();

        let installed = staging.join(executable_name("deno"));
        assert_eq!(fs::read(installed).unwrap(), b"deno-binary");
        assert!(!directory.path().join("escape").exists());
    }

    #[test]
    fn missing_archive_entries_fail_the_component() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("tools.tar.xz");
        {
            let encoder = liblzma::write::XzEncoder::new(File::create(&archive).unwrap(), 1);
            let mut builder = tar::Builder::new(encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(6);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "pkg/bin/ffmpeg", &b"ffmpeg"[..])
                .unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        let staging = directory.path().join("staging");
        fs::create_dir(&staging).unwrap();
        let mut artifact = Artifact {
            url: "https://example.test/tools.tar.xz".into(),
            sha256: String::new(),
            size: 0,
            archive: ArchiveKind::TarXz,
            entries: BTreeMap::from([("ffmpeg".into(), "pkg/bin/ffmpeg".into())]),
        };
        extract(&artifact, &archive, &staging).unwrap();
        assert_eq!(
            fs::read(staging.join(executable_name("ffmpeg"))).unwrap(),
            b"ffmpeg"
        );

        artifact
            .entries
            .insert("ffprobe".into(), "pkg/bin/ffprobe".into());
        assert!(extract(&artifact, &archive, &staging).is_err());
    }

    #[test]
    fn reports_missing_and_outdated_components() {
        let directory = tempfile::tempdir().unwrap();
        if current_target().is_none() {
            return;
        }
        assert_eq!(outdated_components(directory.path()).len(), 3);

        for tool in ["yt-dlp", "deno", "ffmpeg", "ffprobe"] {
            fs::write(directory.path().join(executable_name(tool)), b"").unwrap();
        }
        for component in &manifest().components {
            record_installed(directory.path(), &component.id, &component.version).unwrap();
        }
        assert!(outdated_components(directory.path()).is_empty());

        record_installed(directory.path(), "yt-dlp", "2026.07.04").unwrap();
        let outdated = outdated_components(directory.path());
        assert_eq!(outdated.len(), 1);
        assert_eq!(outdated[0].id, "yt-dlp");
    }
}
