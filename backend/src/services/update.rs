// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Update check and self-update from the GitHub releases.
//!
//! The check only asks GitHub for the latest release. An update downloads the package for this
//! platform, verifies its SHA-256 checksum and then either
//! - runs the new setup (Windows installation), which replaces the program and starts it again, or
//! - replaces the program files next to the running one (portable zip / tar.gz) and starts the new
//!   version. The running file is renamed to `*.old` first, which also works on Windows.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::util::now_str;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const LATEST_URL: &str = "https://api.github.com/repos/jasal82/sorobo/releases/latest";
/// Set for the program started after an update; it waits until the old one released the port
/// (read as `crate::env_var("RESTARTING")`, which also accepts the former `SQONTO_RESTARTING`).
pub const RESTART_ENV: &str = "SOROBO_RESTARTING";

/// Program files as shipped by build.cmd / build.sh – the desktop app first.
#[cfg(windows)]
const BINARIES: [&str; 2] = ["Sorobo.exe", "sorobo-server.exe"];
#[cfg(not(windows))]
const BINARIES: [&str; 2] = ["sorobo", "sorobo-server"];
/// The same programs under their name until version 0.10 ("Sqonto"). Such an installation keeps
/// its file names when it is updated – shortcuts and scripts point at them.
#[cfg(windows)]
const LEGACY_BINARIES: [&str; 2] = ["Sqonto.exe", "sqonto-server.exe"];
#[cfg(not(windows))]
const LEGACY_BINARIES: [&str; 2] = ["sqonto", "sqonto-server"];
const DOCS: [&str; 2] = ["LICENSE.txt", "THIRD-PARTY-LICENSES.txt"];
/// Package names begin with the program name. Releases made during the transition carry the
/// packages under the former name as well, for installations that still look for those.
const PACKAGE_PREFIXES: [&str; 2] = ["Sorobo", "Sqonto"];

/// Former name of a program file, which a package may still use.
fn legacy_name(name: &str) -> Option<&'static str> {
    BINARIES.iter().position(|b| *b == name).map(|i| LEGACY_BINARIES[i])
}

#[derive(Debug, Clone, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

impl Release {
    fn version(&self) -> &str {
        self.tag_name.trim_start_matches('v')
    }

    fn is_update(&self) -> bool {
        !self.draft && !self.prerelease && is_newer(self.version(), VERSION)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Asset {
    name: String,
    #[serde(default)]
    size: u64,
    browser_download_url: String,
    /// `sha256:<hex>`, published by GitHub for each uploaded file.
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct UpdateStatus {
    pub current: String,
    /// Latest published version, once checked.
    pub latest: Option<String>,
    pub available: bool,
    pub published_at: Option<String>,
    /// Release notes (Markdown from GitHub).
    pub notes: String,
    pub release_url: String,
    pub checked_at: Option<String>,
    /// Why the last check failed.
    pub check_error: String,
    /// Sorobo can install the update itself; otherwise it is downloaded from `release_url`.
    pub can_install: bool,
    /// How the update is installed, or why Sorobo cannot do it itself.
    pub install_note: String,
    #[ts(type = "number")]
    pub download_size: u64,
    /// idle | downloading | installing | restarting | failed
    pub phase: String,
    /// Download progress 0…1.
    pub progress: f64,
    /// Why the last installation failed.
    pub error: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Downloading,
    Installing,
    Restarting,
    Failed,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Downloading => "downloading",
            Phase::Installing => "installing",
            Phase::Restarting => "restarting",
            Phase::Failed => "failed",
        }
    }

    fn busy(self) -> bool {
        matches!(self, Phase::Downloading | Phase::Installing | Phase::Restarting)
    }
}

struct State {
    release: Option<Release>,
    checked_at: Option<String>,
    check_error: String,
    phase: Phase,
    done: u64,
    total: u64,
    error: String,
}

static STATE: LazyLock<Mutex<State>> =
    LazyLock::new(|| Mutex::new(State { release: None, checked_at: None, check_error: String::new(), phase: Phase::Idle, done: 0, total: 0, error: String::new() }));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    /// Windows installation made by the setup (Inno Setup).
    Installer,
    /// Portable zip or tar.gz: the files next to the program are replaced.
    Replace,
}

#[derive(Debug, Clone)]
struct Target {
    method: Method,
    exe: PathBuf,
    dir: PathBuf,
    /// File names of the programs in this installation: `BINARIES`, or `LEGACY_BINARIES` for one
    /// made before the rename.
    names: [&'static str; 2],
}

/// How this installation can be updated – determined once, the program file does not move.
static TARGET: LazyLock<Result<Target, String>> = LazyLock::new(detect);

fn detect() -> Result<Target, String> {
    if cfg!(debug_assertions) {
        return Err("Entwicklungsversion – Updates bitte aus dem Quellcode bauen.".into());
    }
    let exe = std::env::current_exe().map_err(|e| format!("Die Programmdatei wurde nicht gefunden ({e})."))?;
    let dir = exe.parent().map(Path::to_path_buf).ok_or("Der Programmordner wurde nicht gefunden.")?;
    let name = exe.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let names = if BINARIES.contains(&name) {
        BINARIES
    } else if LEGACY_BINARIES.contains(&name) {
        LEGACY_BINARIES
    } else {
        return Err(format!("„{name}“ stammt nicht aus einem Sorobo-Paket – bitte manuell aktualisieren."));
    };
    if cfg!(windows) && dir.join("unins000.exe").exists() {
        return Ok(Target { method: Method::Installer, exe, dir, names });
    }
    if !writable(&dir) {
        return Err(format!("Keine Schreibrechte im Programmordner {} – bitte manuell aktualisieren.", dir.display()));
    }
    Ok(Target { method: Method::Replace, exe, dir, names })
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".sorobo-schreibtest-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// `1.2.3` (an optional leading `v` is ignored); pre-releases like `1.2.3-rc1` are not offered.
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(l), Some(c)) if l > c)
}

/// Package names for this platform, preferred first (see build.cmd, build.sh and the release workflow).
fn package_names(version: &str, method: Method) -> Vec<String> {
    PACKAGE_PREFIXES
        .iter()
        .flat_map(|prefix| {
            let base = format!("{prefix}-{version}");
            match (std::env::consts::OS, std::env::consts::ARCH) {
                ("windows", "x86_64") if method == Method::Installer => vec![format!("{base}-windows-x64-setup.exe")],
                ("windows", "x86_64") => vec![format!("{base}-windows-x64-portable.zip")],
                ("windows", _) => vec![],
                (os, arch) => {
                    // build.sh names the archives after `uname -s` and `uname -m`.
                    let os = if os == "macos" { "darwin" } else { os };
                    let arch = if os == "darwin" && arch == "aarch64" { "arm64" } else { arch };
                    let full = format!("{base}-{os}-{arch}.tar.gz");
                    // A server-only build prefers its own package; from the full one only the server is taken.
                    if cfg!(feature = "desktop") { vec![full] } else { vec![format!("{base}-{os}-{arch}-server.tar.gz"), full] }
                }
            }
        })
        .collect()
}

fn plan(r: &Release) -> Result<(Target, Asset), String> {
    let target = TARGET.clone()?;
    package_names(r.version(), target.method)
        .iter()
        .find_map(|n| r.assets.iter().find(|a| &a.name == n))
        .cloned()
        .map(|a| (target, a))
        .ok_or_else(|| "Für dieses System gibt es kein passendes Paket – bitte manuell aktualisieren.".into())
}

pub fn status() -> UpdateStatus {
    let s = STATE.lock();
    let mut out = UpdateStatus {
        current: VERSION.into(),
        checked_at: s.checked_at.clone(),
        check_error: s.check_error.clone(),
        phase: s.phase.as_str().into(),
        progress: if s.total > 0 { (s.done as f64 / s.total as f64).min(1.0) } else { 0.0 },
        error: s.error.clone(),
        ..Default::default()
    };
    let Some(r) = &s.release else { return out };
    out.latest = Some(r.version().to_string());
    out.available = r.is_update();
    out.published_at = r.published_at.clone();
    out.notes = r.body.clone().unwrap_or_default();
    out.release_url = r.html_url.clone();
    if out.available {
        match plan(r) {
            Ok((t, a)) => {
                out.can_install = true;
                out.download_size = a.size;
                out.install_note = match t.method {
                    Method::Installer => "Das Installationsprogramm der neuen Version wird heruntergeladen und ausgeführt.".into(),
                    Method::Replace => format!("Die Programmdateien in {} werden ersetzt.", t.dir.display()),
                };
            }
            Err(e) => out.install_note = e,
        }
    }
    out
}

/// Asks GitHub for the latest release; a failure is kept in the status.
pub fn check() -> UpdateStatus {
    let result = fetch_release();
    {
        let mut s = STATE.lock();
        s.checked_at = Some(now_str());
        match result {
            Ok(r) => {
                s.release = Some(r);
                s.check_error.clear();
            }
            Err(e) => {
                tracing::info!("Update-Prüfung fehlgeschlagen: {e:#}");
                s.check_error = format!("{e:#}");
            }
        }
    }
    status()
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(format!("Sorobo/{VERSION}"))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(timeout))
        // The system's certificates, so that company proxies with an own CA keep working.
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build())
        .build()
        .into()
}

fn fetch_release() -> Result<Release> {
    // For tests against a local copy of the API.
    let url = crate::env_var("UPDATE_URL").unwrap_or_else(|| LATEST_URL.into());
    let resp = agent(Duration::from_secs(30)).get(&url).header("Accept", "application/vnd.github+json").call().map_err(|e| match e {
        ureq::Error::StatusCode(404) => anyhow!("Es wurde noch keine Version veröffentlicht."),
        ureq::Error::StatusCode(403 | 429) => anyhow!("GitHub lehnt die Abfrage gerade ab (Abfragelimit) – bitte später erneut versuchen."),
        e => anyhow!("GitHub ist nicht erreichbar ({e})."),
    })?;
    serde_json::from_reader(resp.into_body().into_reader()).context("Die Antwort von GitHub ist unverständlich")
}

/// Starts an installation: only one at a time, and only of an update this program can install.
pub fn begin() -> Result<(), String> {
    let mut s = STATE.lock();
    if s.phase.busy() {
        return Err("Das Update wird bereits installiert.".into());
    }
    let r = s.release.as_ref().ok_or("Bitte zuerst nach Updates suchen.")?;
    if !r.is_update() {
        return Err("Sorobo ist bereits auf dem neuesten Stand.".into());
    }
    plan(r)?;
    s.phase = Phase::Downloading;
    s.done = 0;
    s.total = 0;
    s.error.clear();
    Ok(())
}

pub fn set_phase(phase: Phase) {
    STATE.lock().phase = phase;
}

pub fn fail(message: String) {
    let mut s = STATE.lock();
    s.phase = Phase::Failed;
    s.error = message;
}

/// Downloads, verifies and installs the update into place. Returns the command that starts the new
/// version – or the setup, which installs and then starts it. The caller ends this program.
pub fn install(work: &Path) -> Result<Command> {
    let release = STATE.lock().release.clone().context("Bitte zuerst nach Updates suchen.")?;
    let (target, asset) = plan(&release).map_err(|e| anyhow!(e))?;
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work)?;
    let expected = expected_sha256(&release, &asset)?;
    let package = work.join(&asset.name);
    download(&asset, &package, &expected)?;
    set_phase(Phase::Installing);
    match target.method {
        Method::Installer => Ok(setup_command(&package, &target)),
        Method::Replace => {
            let staged = work.join("neu");
            let files = files_to_replace(&target);
            unpack(&package, &staged, &files)?;
            let names: Vec<&str> = files.iter().map(|(_, installed)| *installed).collect();
            replace_files(&staged, &target.dir, &names)?;
            Ok(restart_command(&target.exe))
        }
    }
}

/// SHA-256 of the package as published by GitHub, or from the release's SHA256SUMS.txt.
fn expected_sha256(r: &Release, a: &Asset) -> Result<String> {
    let hex = match a.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
        Some(hex) => hex.to_ascii_lowercase(),
        None => {
            let sums = r.assets.iter().find(|x| x.name == "SHA256SUMS.txt").context("Für das Paket ist keine Prüfsumme veröffentlicht – Update abgebrochen.")?;
            let text = agent(Duration::from_secs(60)).get(&sums.browser_download_url).call()?.into_body().read_to_string()?;
            sum_for(&text, &a.name).context("Das Paket fehlt in SHA256SUMS.txt – Update abgebrochen.")?
        }
    };
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("Die veröffentlichte Prüfsumme ist ungültig – Update abgebrochen.");
    }
    Ok(hex)
}

/// Line of `sha256sum` output for `name` (`<hex>  <name>`, binary mode `<hex> *<name>`).
fn sum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| hash.to_ascii_lowercase())
    })
}

fn download(a: &Asset, file: &Path, expected: &str) -> Result<()> {
    let resp = agent(Duration::from_secs(3600)).get(&a.browser_download_url).call().map_err(|e| anyhow!("Der Download ist fehlgeschlagen ({e})."))?;
    STATE.lock().total = resp.body().content_length().unwrap_or(a.size);
    let mut reader = resp.into_body().into_reader();
    let mut out = File::create(file)?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).context("Der Download wurde unterbrochen")?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        hash.update(&buf[..n]);
        done += n as u64;
        STATE.lock().done = done;
    }
    out.sync_all()?;
    drop(out);
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != expected {
        let _ = std::fs::remove_file(file);
        bail!("Die Prüfsumme des Downloads stimmt nicht – Update abgebrochen.");
    }
    Ok(())
}

/// The program files of this installation (a server-only setup stays one) and the licence texts,
/// each as (name in the package, name in the installation).
fn files_to_replace(t: &Target) -> Vec<(&'static str, &'static str)> {
    (0..BINARIES.len())
        .filter(|&i| t.dir.join(t.names[i]).exists() || t.exe.ends_with(t.names[i]))
        .map(|i| (BINARIES[i], t.names[i]))
        .chain(DOCS.into_iter().map(|d| (d, d)))
        .collect()
}

/// Extracts the given files from the top level of the package (zip or tar.gz) under their name in
/// the installation – nothing else, so a package cannot write anywhere but `dest`. A program file
/// may also carry its former name. Every program file must be contained.
fn unpack(package: &Path, dest: &Path, files: &[(&str, &str)]) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    let wanted = |path: &str| files.iter().find(|(packaged, _)| *packaged == path || legacy_name(packaged) == Some(path)).map(|(_, installed)| *installed);
    if package.extension().is_some_and(|e| e == "zip") {
        let mut zip = zip::ZipArchive::new(File::open(package)?).context("Das Paket ist beschädigt")?;
        for (packaged, installed) in files {
            if let Some(i) = std::iter::once(*packaged).chain(legacy_name(packaged)).find_map(|n| zip.index_for_name(n)) {
                std::io::copy(&mut zip.by_index(i)?, &mut File::create(dest.join(installed))?)?;
            }
        }
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(File::open(package)?));
        for entry in tar.entries().context("Das Paket ist beschädigt")? {
            let mut e = entry.context("Das Paket ist beschädigt")?;
            let path = e.path()?.to_string_lossy().trim_start_matches("./").to_string();
            if e.header().entry_type().is_file()
                && let Some(installed) = wanted(&path)
            {
                std::io::copy(&mut e, &mut File::create(dest.join(installed))?)?;
            }
        }
    }
    for (packaged, installed) in files.iter().filter(|(packaged, _)| BINARIES.contains(packaged)) {
        if !dest.join(installed).is_file() {
            bail!("Das Paket enthält {packaged} nicht – Update abgebrochen.");
        }
    }
    Ok(())
}

/// Puts the new files in place. The current ones are renamed to `*.old` first (a running program
/// can be renamed, but not overwritten); after any error the previous state is restored.
fn replace_files(staged: &Path, dir: &Path, names: &[&str]) -> Result<()> {
    let mut touched: Vec<&str> = Vec::new();
    let result = (|| -> Result<()> {
        for n in names {
            let new = staged.join(n);
            if !new.is_file() {
                continue;
            }
            let (current, old) = (dir.join(n), dir.join(format!("{n}.old")));
            let _ = std::fs::remove_file(&old);
            if current.exists() {
                std::fs::rename(&current, &old).with_context(|| format!("{} kann nicht ersetzt werden", current.display()))?;
            }
            touched.push(n);
            std::fs::copy(&new, &current).with_context(|| format!("{} kann nicht geschrieben werden", current.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if BINARIES.contains(n) || LEGACY_BINARIES.contains(n) {
                    std::fs::set_permissions(&current, std::fs::Permissions::from_mode(0o755))?;
                }
            }
        }
        Ok(())
    })();
    if result.is_err() {
        for n in touched.iter().rev() {
            let (current, old) = (dir.join(n), dir.join(format!("{n}.old")));
            let _ = std::fs::remove_file(&current);
            if old.exists() {
                let _ = std::fs::rename(&old, &current);
            }
        }
    }
    result
}

/// The setup updates the installation without questions (showing its progress) and starts the
/// updated program again – `/RELAUNCH` is read by installer/sorobo.iss.
fn setup_command(setup: &Path, t: &Target) -> Command {
    let relaunch = if t.exe.ends_with(t.names[0]) { "desktop" } else { "server" };
    let mut c = Command::new(setup);
    c.args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"]).arg(format!("/RELAUNCH={relaunch}"));
    c
}

/// Starts the replaced program with the same arguments; the open page reloads itself, so no
/// second browser tab is opened.
fn restart_command(exe: &Path) -> Command {
    let mut c = Command::new(exe);
    c.args(std::env::args_os().skip(1).filter(|a| a != "--open")).env(RESTART_ENV, "1");
    c
}

/// Removes what an earlier update left behind: the downloaded package and the replaced files.
pub fn cleanup(work: &Path) {
    if STATE.lock().phase.busy() {
        return;
    }
    let _ = std::fs::remove_dir_all(work);
    if let Ok(t) = &*TARGET {
        for n in BINARIES.iter().chain(&LEGACY_BINARIES).chain(&DOCS) {
            let _ = std::fs::remove_file(t.dir.join(format!("{n}.old")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sorobo-update-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn versions_are_compared_numerically() {
        assert!(is_newer("0.10.0", "0.9.1"));
        assert!(is_newer("v1.0.0", "0.9.1"));
        assert!(!is_newer("0.9.1", "0.9.1"));
        assert!(!is_newer("0.9.0", "0.9.1"));
        assert!(!is_newer("1.0.0-rc1", "0.9.1"));
        assert!(!is_newer("1.0", "0.9.1"));
    }

    #[test]
    fn drafts_and_prereleases_are_no_updates() {
        let r = Release { tag_name: "v99.0.0".into(), body: None, html_url: String::new(), published_at: None, draft: false, prerelease: false, assets: vec![] };
        assert!(r.is_update());
        assert!(!Release { prerelease: true, ..r.clone() }.is_update());
        assert!(!Release { draft: true, ..r }.is_update());
    }

    #[test]
    fn packages_match_the_release_assets() {
        let names = package_names("1.2.3", Method::Replace);
        if cfg!(windows) {
            assert_eq!(names, ["Sorobo-1.2.3-windows-x64-portable.zip", "Sqonto-1.2.3-windows-x64-portable.zip"]);
            assert_eq!(package_names("1.2.3", Method::Installer), ["Sorobo-1.2.3-windows-x64-setup.exe", "Sqonto-1.2.3-windows-x64-setup.exe"]);
        } else {
            let platform = if cfg!(target_os = "macos") { "darwin-arm64" } else { "linux-x86_64" };
            // The new name first; the former one only as a fallback.
            assert!(names.first().unwrap().starts_with("Sorobo-1.2.3-"));
            assert!(names.contains(&format!("Sorobo-1.2.3-{platform}.tar.gz")));
            assert_eq!(names.last().unwrap(), &format!("Sqonto-1.2.3-{platform}.tar.gz"));
        }
    }

    #[test]
    fn checksums_are_found_in_sha256sums() {
        let sums = "861cce  SHA256SUMS.txt\nABC123  Sorobo-1.0.0-windows-x64-setup.exe\ndef456 *Sorobo-1.0.0-linux-x86_64.tar.gz\n";
        assert_eq!(sum_for(sums, "Sorobo-1.0.0-windows-x64-setup.exe").as_deref(), Some("abc123"));
        assert_eq!(sum_for(sums, "Sorobo-1.0.0-linux-x86_64.tar.gz").as_deref(), Some("def456"));
        assert_eq!(sum_for(sums, "Sorobo-1.0.0-windows-x64-portable.zip"), None);
    }

    #[test]
    fn only_the_program_files_are_unpacked() {
        let dir = temp_dir("unpack");
        let names = [(BINARIES[0], BINARIES[0]), (BINARIES[1], BINARIES[1]), (DOCS[0], DOCS[0])];
        // zip as made by Compress-Archive
        let zip_path = dir.join("p.zip");
        let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
        for (n, body) in [(BINARIES[0], "desktop"), (BINARIES[1], "server"), ("data/sorobo.db", "fremd")] {
            zip.start_file(n, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        unpack(&zip_path, &dir.join("z"), &names).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("z").join(BINARIES[1])).unwrap(), "server");
        assert!(!dir.join("z").join("data").exists());
        // tar.gz as made by build.sh
        let tar_path = dir.join("p.tar.gz");
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(File::create(&tar_path).unwrap(), flate2::Compression::fast()));
        for (n, body) in [(format!("./{}", BINARIES[0]), "desktop"), (BINARIES[1].to_string(), "server"), (format!("data/{}", BINARIES[1]), "fremd")] {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            tar.append_data(&mut h, n, body.as_bytes()).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
        unpack(&tar_path, &dir.join("t"), &names).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("t").join(BINARIES[0])).unwrap(), "desktop");
        assert_eq!(std::fs::read_to_string(dir.join("t").join(BINARIES[1])).unwrap(), "server");
        assert!(!dir.join("t").join("data").exists());
        // A package without the server is refused.
        let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
        zip.start_file(BINARIES[0], zip::write::SimpleFileOptions::default()).unwrap();
        zip.finish().unwrap();
        assert!(unpack(&zip_path, &dir.join("z2"), &names).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn zip_with(path: &Path, files: &[(&str, &str)]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (n, body) in files {
            zip.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn an_installation_from_before_the_rename_keeps_its_file_names() {
        let dir = temp_dir("legacy");
        let exe = dir.join(LEGACY_BINARIES[0]);
        std::fs::write(&exe, "alt").unwrap();
        std::fs::write(dir.join(LEGACY_BINARIES[1]), "alt").unwrap();
        let target = Target { method: Method::Replace, exe, dir: dir.clone(), names: LEGACY_BINARIES };
        let files = files_to_replace(&target);
        assert_eq!(&files[..2], [(BINARIES[0], LEGACY_BINARIES[0]), (BINARIES[1], LEGACY_BINARIES[1])]);
        // A package with the new names is installed under the former ones.
        let package = dir.join("neu.zip");
        zip_with(&package, &[(BINARIES[0], "desktop"), (BINARIES[1], "server")]);
        let staged = dir.join("neu");
        unpack(&package, &staged, &files).unwrap();
        let names: Vec<&str> = files.iter().map(|(_, installed)| *installed).collect();
        replace_files(&staged, &dir, &names).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(LEGACY_BINARIES[0])).unwrap(), "desktop");
        assert_eq!(std::fs::read_to_string(dir.join(LEGACY_BINARIES[1])).unwrap(), "server");
        assert!(!dir.join(BINARIES[0]).exists());
        assert!(setup_command(Path::new("setup.exe"), &target).get_args().any(|a| a == "/RELAUNCH=desktop"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_package_may_still_use_the_former_file_names() {
        let dir = temp_dir("legacy-package");
        let files = [(BINARIES[0], BINARIES[0]), (BINARIES[1], BINARIES[1])];
        let package = dir.join("alt.zip");
        zip_with(&package, &[(LEGACY_BINARIES[0], "desktop"), (LEGACY_BINARIES[1], "server")]);
        unpack(&package, &dir.join("z"), &files).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("z").join(BINARIES[0])).unwrap(), "desktop");
        assert_eq!(std::fs::read_to_string(dir.join("z").join(BINARIES[1])).unwrap(), "server");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_are_replaced_and_restored_on_error() {
        let dir = temp_dir("replace");
        let (app, staged) = (dir.join("app"), dir.join("neu"));
        std::fs::create_dir_all(&app).unwrap();
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(app.join(BINARIES[0]), "alt").unwrap();
        std::fs::write(app.join(BINARIES[1]), "alt").unwrap();
        std::fs::write(staged.join(BINARIES[0]), "neu").unwrap();
        std::fs::write(staged.join(BINARIES[1]), "neu").unwrap();
        replace_files(&staged, &app, &BINARIES).unwrap();
        assert_eq!(std::fs::read_to_string(app.join(BINARIES[0])).unwrap(), "neu");
        assert_eq!(std::fs::read_to_string(app.join(format!("{}.old", BINARIES[1]))).unwrap(), "alt");

        // The second file cannot be written (its folder is missing): the first one is put back.
        std::fs::write(staged.join(BINARIES[0]), "neuer").unwrap();
        std::fs::create_dir_all(staged.join("fehlt")).unwrap();
        std::fs::write(staged.join("fehlt").join("x"), "neu").unwrap();
        assert!(replace_files(&staged, &app, &[BINARIES[0], "fehlt/x"]).is_err());
        assert_eq!(std::fs::read_to_string(app.join(BINARIES[0])).unwrap(), "neu");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
