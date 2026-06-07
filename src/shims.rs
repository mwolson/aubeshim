use crate::home_dir;
use anyhow::{bail, Context, Result};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

const SHIM_NAMES: &[&str] = &["bun", "bunx", "npm", "npx", "pnpm", "pnpx", "pnx", "yarn"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invocation {
    Manager,
    Shim(ShimTool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShimTool {
    Bun,
    Bunx,
    Npm,
    Npx,
    Pnpm,
    Pnpx,
    Pnx,
    Yarn,
}

pub fn invocation_from_argv0(argv0: Option<&OsString>) -> Invocation {
    let Some(argv0) = argv0 else {
        return Invocation::Manager;
    };
    let stem = Path::new(argv0)
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("aubeshim")
        .to_ascii_lowercase();
    match stem.as_str() {
        "bun" => Invocation::Shim(ShimTool::Bun),
        "bunx" => Invocation::Shim(ShimTool::Bunx),
        "npm" => Invocation::Shim(ShimTool::Npm),
        "npx" => Invocation::Shim(ShimTool::Npx),
        "pnpm" => Invocation::Shim(ShimTool::Pnpm),
        "pnpx" => Invocation::Shim(ShimTool::Pnpx),
        "pnx" => Invocation::Shim(ShimTool::Pnx),
        "yarn" => Invocation::Shim(ShimTool::Yarn),
        _ => Invocation::Manager,
    }
}
pub fn default_shim_dir() -> PathBuf {
    if let Some(dir) = env::var_os("AUBESHIM_SHIM_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(data_home).join("aubeshim").join("shims");
    }
    home_dir().join(".local/share/aubeshim/shims")
}
pub fn install_shims(shim_dir: &Path, force: bool) -> Result<Vec<PathBuf>> {
    let exe = env::current_exe().context("could not locate current executable")?;
    fs::create_dir_all(shim_dir)
        .with_context(|| format!("could not create {}", shim_dir.display()))?;

    let mut installed = Vec::new();
    for name in SHIM_NAMES {
        let path = shim_dir.join(name);
        if path.exists() || path.is_symlink() {
            if !force {
                bail!(
                    "{} already exists; rerun with force to replace it",
                    path.display()
                );
            }
            fs::remove_file(&path)
                .with_context(|| format!("could not remove {}", path.display()))?;
        }
        link_executable(&exe, &path)
            .with_context(|| format!("could not install {}", path.display()))?;
        installed.push(path);
    }
    Ok(installed)
}

pub fn uninstall_shims(shim_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for name in SHIM_NAMES {
        let path = shim_dir.join(name);
        if path.exists() || path.is_symlink() {
            fs::remove_file(&path)
                .with_context(|| format!("could not remove {}", path.display()))?;
            removed.push(path);
        }
    }
    Ok(removed)
}
fn link_executable(src: &Path, dest: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dest)?;
    }

    #[cfg(windows)]
    {
        fs::copy(src, dest)?;
    }

    Ok(())
}

pub(crate) fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}
