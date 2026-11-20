//! Running programs, downloads, directories.

use std::path::Path;
use std::process::{Command, Stdio};

pub fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd
        .status()
        .map_err(|e| format!("{:?}: {e}", cmd.get_program()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{:?} failed: {status}", cmd.get_program()))
    }
}

/// Its standard output; its errors go to ours.
pub fn stdout(cmd: &mut Command) -> Result<String, String> {
    let out = cmd
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("{:?}: {e}", cmd.get_program()))?;
    if !out.status.success() {
        return Err(format!("{:?} failed: {}", cmd.get_program(), out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn fetch(url: &str) -> Result<String, String> {
    stdout(Command::new("curl").args(["-fsSL", url])).map_err(|e| format!("{url}: {e}"))
}

pub fn mkdir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}
