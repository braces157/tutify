use super::{Level, Report, hash_file};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

fn resolve(directories: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    directories
        .into_iter()
        .filter(|directory| !directory.as_os_str().is_empty())
        .map(|directory| directory.join("tuitify.exe"))
        .find(|file| file.is_file())
}

fn path_directories(path: &OsStr) -> Vec<PathBuf> {
    std::env::split_paths(path).collect()
}

fn same_directory(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_owned()
    };
    normalize(left).eq_ignore_ascii_case(&normalize(right))
}

fn compare(report: &mut Report, name: &str, candidate: Option<&Path>, running_hash: Option<&str>) {
    match candidate {
        Some(path) => match hash_file(path) {
            Ok(hash) if Some(hash.as_str()) == running_hash => report.add(name, Level::Pass, format!("{} · SHA-256 {hash} matches this executable on disk.", path.display()), None),
            Ok(hash) if running_hash.is_some() => report.add(name, Level::Failure, format!("{} · SHA-256 {hash} differs from this executable on disk.", path.display()), Some("Install the intended release with scripts/install.ps1, update conflicting launch copies using -InstallDir, run the canonical install last, and reopen your terminal/player.")),
            Ok(hash) => report.add(name, Level::Unknown, format!("{} · SHA-256 {hash}; running-executable comparison unavailable.", path.display()), Some("Check executable read permissions and rerun doctor.")),
            Err(_) => report.add(name, Level::Failure, "Executable exists but its SHA-256 could not be read.", Some("Check executable access permissions and reinstall the intended release if needed.")),
        },
        None => report.add(name, Level::Warning, "No executable found at this location or search path.", Some("Install with scripts/install.ps1, then open a fresh terminal and check Get-Command tuitify -All and where.exe tuitify.")),
    }
}

pub(super) fn checks(report: &mut Report) {
    let running = std::env::current_exe().ok();
    let running_hash = running.as_deref().and_then(|path| hash_file(path).ok());
    match (&running, &running_hash) {
        (Some(path), Some(hash)) => report.add("installation.running", Level::Pass, format!("{} · SHA-256 {hash}. An already-running older process must be restarted after installation.", path.display()), None),
        _ => report.add("installation.running", Level::Failure, "Cannot resolve or hash this executable on disk.", Some("Check executable access permissions and reinstall the intended release if needed.")),
    }
    let process_path = std::env::var_os("PATH").unwrap_or_default();
    let resolved = resolve(path_directories(&process_path));
    compare(
        report,
        "installation.process_path",
        resolved.as_deref(),
        running_hash.as_deref(),
    );
    let current_directory_copy = std::env::current_dir()
        .ok()
        .map(|directory| directory.join("tuitify.exe"))
        .filter(|file| file.is_file());
    if let Some(copy) = current_directory_copy.as_deref() {
        compare(
            report,
            "installation.current_directory",
            Some(copy),
            running_hash.as_deref(),
        );
        report.add("installation.shell_lookup", Level::Unknown, "where.exe/cmd can prefer the current-directory executable; PowerShell Get-Command uses its own command lookup. Aliases/functions cannot be inspected from this child process.", Some("In the terminal you use to launch Tuitify, compare Get-Command tuitify -All and where.exe tuitify."));
    }
    let canonical = std::env::var_os("LOCALAPPDATA")
        .map(|directory| PathBuf::from(directory).join("Programs/Tuitify/tuitify.exe"));
    compare(
        report,
        "installation.canonical",
        canonical.as_deref().filter(|path| path.is_file()),
        running_hash.as_deref(),
    );
    match saved_paths() {
        Ok((machine, user)) => {
            let mut directories = path_directories(OsStr::new(&machine));
            directories.extend(path_directories(OsStr::new(&user)));
            let saved_resolved = resolve(directories);
            compare(report, "installation.saved_path", saved_resolved.as_deref(), running_hash.as_deref());
            if let Some(canonical) = canonical.as_deref().and_then(Path::parent) {
                let first = path_directories(OsStr::new(&user)).into_iter().find(|path| !path.as_os_str().is_empty());
                let canonical_first = first.as_deref().is_some_and(|first| same_directory(first, canonical));
                report.add("installation.user_path_order", if canonical_first { Level::Pass } else { Level::Warning }, if canonical_first { "Canonical Tuitify directory is first in saved user PATH." } else { "Canonical Tuitify directory is not first in saved user PATH; existing terminals may also have stale PATH values." }, (!canonical_first).then_some("Run the canonical scripts/install.ps1 installation last, preserving unrelated PATH entries; reopen the terminal."));
            }
        }
        Err(()) => report.add("installation.saved_path", Level::Unknown, "Cannot read saved Windows Machine/User PATH; process PATH alone does not prove fresh-terminal resolution.", Some("Inspect [Environment]::GetEnvironmentVariable('Path', 'User') and Get-Command tuitify -All in a newly opened PowerShell terminal.")),
    }
}

#[cfg(windows)]
fn saved_paths() -> Result<(String, String), ()> {
    use windows::{
        Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
            RegGetValueW,
        },
        core::w,
    };
    fn read(key: HKEY, subkey: windows::core::PCWSTR) -> Result<String, ()> {
        // Windows environment values are bounded by 32,767 UTF-16 characters.
        // RegGetValueW is read-only and expands REG_EXPAND_SZ without changing
        // this process's environment or invoking a shell.
        let mut buffer = vec![0_u16; 32_768];
        let mut size = (buffer.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                key,
                subkey,
                w!("Path"),
                RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status.0 == 2 {
            return Ok(String::new());
        } // absent value
        if status.0 != 0 || size as usize > buffer.len() * 2 || !size.is_multiple_of(2) {
            return Err(());
        }
        let length = buffer
            .iter()
            .take(size as usize / 2)
            .position(|ch| *ch == 0)
            .ok_or(())?;
        String::from_utf16(&buffer[..length]).map_err(|_| ())
    }
    Ok((
        read(
            HKEY_LOCAL_MACHINE,
            w!("SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment"),
        )?,
        read(HKEY_CURRENT_USER, w!("Environment"))?,
    ))
}

#[cfg(not(windows))]
fn saved_paths() -> Result<(String, String), ()> {
    Err(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_path_order_comparison_accepts_slash_case_and_trailing_separators() {
        assert!(same_directory(
            Path::new("C:/Users/PC/AppData/Local/Programs/Tuitify"),
            Path::new("c:\\users\\pc\\appdata\\local\\programs\\tuitify\\")
        ));
        assert!(!same_directory(
            Path::new("C:/Programs/Tuitify"),
            Path::new("C:/Programs/Tuitify/bin")
        ));
    }
    #[test]
    fn path_order_and_hashes_detect_conflicting_same_version_copies() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("tuitify.exe"), "old release with same version").unwrap();
        std::fs::write(second.join("tuitify.exe"), "latest release").unwrap();
        let resolved = resolve([PathBuf::new(), first.clone(), second.clone()]).unwrap();
        assert_eq!(resolved, first.join("tuitify.exe"));
        let expected = hash_file(&second.join("tuitify.exe")).unwrap();
        let mut report = Report::default();
        compare(&mut report, "old", Some(&resolved), Some(&expected));
        compare(
            &mut report,
            "new",
            Some(&second.join("tuitify.exe")),
            Some(&expected),
        );
        assert_eq!(report.checks[0].level, Level::Failure);
        assert_eq!(report.checks[1].level, Level::Pass);
        assert!(
            report.checks[0]
                .action
                .as_ref()
                .unwrap()
                .contains("-InstallDir")
        );
        assert_eq!(
            std::fs::read_to_string(&resolved).unwrap(),
            "old release with same version"
        );
    }
}
