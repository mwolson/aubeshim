use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitStatus};

const SHIM_NAMES: &[&str] = &["npm", "pnpm"];

#[derive(Debug, Parser)]
#[command(
    name = "aubeshim",
    version,
    about = "Install and run aube-backed package-manager shims"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print shell code that prepends the aubeshim shim directory to PATH
    Init {
        /// Shell syntax to emit
        shell: Shell,
        /// Shim directory to put on PATH
        #[arg(long, value_name = "DIR")]
        shim_dir: Option<PathBuf>,
    },
    /// Create npm and pnpm shims that point at this executable
    Install {
        /// Replace existing shim files
        #[arg(long)]
        force: bool,
        /// Directory where package-manager shims should be installed
        #[arg(long, value_name = "DIR")]
        shim_dir: Option<PathBuf>,
    },
    /// Remove npm and pnpm shims
    Uninstall {
        /// Directory where package-manager shims were installed
        #[arg(long, value_name = "DIR")]
        shim_dir: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Bash,
    Fish,
    Sh,
    Zsh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invocation {
    Manager,
    Shim(ShimTool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShimTool {
    Npm,
    Pnpm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub target: Target,
    pub args: Vec<OsString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Aube,
    RealNpm,
    RealPnpm,
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
        "npm" => Invocation::Shim(ShimTool::Npm),
        "pnpm" => Invocation::Shim(ShimTool::Pnpm),
        _ => Invocation::Manager,
    }
}

pub fn exec_shim(tool: ShimTool, args: &[OsString]) -> Result<()> {
    let plan = plan_for(tool, args);
    let status = run_plan(plan)?;
    std::process::exit(exit_code(status));
}

pub fn plan_for(tool: ShimTool, args: &[OsString]) -> Plan {
    match tool {
        ShimTool::Npm => plan_npm(args),
        ShimTool::Pnpm => Plan {
            target: Target::Aube,
            args: args.to_vec(),
        },
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

pub fn shell_init(shell: Shell, shim_dir: &Path) -> String {
    let dir = shell_quote(&shim_dir.to_string_lossy());
    match shell {
        Shell::Bash | Shell::Zsh => format!(
            "_aubeshim_shim_dir={dir}\nPATH=\":$PATH:\"\nPATH=\"${{PATH//:$_aubeshim_shim_dir:/:}}\"\nPATH=\"${{PATH#:}}\"\nPATH=\"${{PATH%:}}\"\nexport PATH=\"$_aubeshim_shim_dir:$PATH\"\nunset _aubeshim_shim_dir\n"
        ),
        Shell::Fish => format!("fish_add_path --prepend {dir}\n"),
        Shell::Sh => format!(
            "AUBESHIM_SHIM_DIR=${{AUBESHIM_SHIM_DIR:-{dir}}}\ncase \":$PATH:\" in\n    *:\"$AUBESHIM_SHIM_DIR\":*) ;;\n    *) PATH=\"$AUBESHIM_SHIM_DIR:$PATH\" ;;\nesac\nexport PATH\n"
        ),
    }
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

fn plan_npm(args: &[OsString]) -> Plan {
    let Some(command_idx) = command_index(args) else {
        return Plan {
            target: Target::Aube,
            args: args.to_vec(),
        };
    };
    let command = args[command_idx].to_string_lossy().to_ascii_lowercase();
    let prefix = &args[..command_idx];
    let rest = &args[command_idx + 1..];

    if npm_only_command(&command) || !known_npm_command(&command) {
        return Plan {
            target: Target::RealNpm,
            args: args.to_vec(),
        };
    }

    if matches!(command.as_str(), "install" | "i" | "in") && install_has_packages(rest) {
        let (prefix, moved) = move_global_markers(prefix);
        let mut out = Vec::with_capacity(args.len());
        out.extend(prefix);
        out.push(OsString::from("add"));
        out.extend(moved);
        out.extend(translate_npm_install_package_args(rest));
        return Plan {
            target: Target::Aube,
            args: out,
        };
    }

    let mut out = Vec::with_capacity(args.len());
    out.extend_from_slice(prefix);
    out.push(OsString::from(normalize_npm_command(&command)));
    out.extend_from_slice(rest);
    Plan {
        target: Target::Aube,
        args: out,
    }
}

fn run_plan(plan: Plan) -> Result<ExitStatus> {
    let program = resolve_target(plan.target)?;
    let mut cmd = ProcessCommand::new(&program);
    cmd.args(&plan.args);

    if matches!(plan.target, Target::Aube) && env::var_os("AUBE_NPM_PATH").is_none() {
        if let Some(npm) = resolve_real_npm() {
            cmd.env("AUBE_NPM_PATH", npm);
        }
    }

    cmd.status()
        .with_context(|| format!("failed to run {}", PathBuf::from(program).display()))
}

fn resolve_target(target: Target) -> Result<OsString> {
    match target {
        Target::Aube => resolve_aube().context("could not find aube"),
        Target::RealNpm => resolve_real_npm().context("could not find real npm"),
        Target::RealPnpm => resolve_real_pnpm().context("could not find real pnpm"),
    }
}

fn resolve_aube() -> Option<OsString> {
    env::var_os("AUBESHIM_AUBE")
        .or_else(|| mise_which("aube"))
        .or_else(|| path_which("aube"))
}

fn resolve_real_npm() -> Option<OsString> {
    env::var_os("AUBESHIM_REAL_NPM")
        .or_else(|| mise_which("npm"))
        .or_else(|| path_which_excluding_shims("npm"))
}

fn resolve_real_pnpm() -> Option<OsString> {
    env::var_os("AUBESHIM_REAL_PNPM")
        .or_else(|| mise_which("pnpm"))
        .or_else(|| path_which_excluding_shims("pnpm"))
}

fn mise_which(tool: &str) -> Option<OsString> {
    let output = ProcessCommand::new("mise")
        .arg("which")
        .arg(tool)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    if path.is_empty() {
        None
    } else {
        Some(OsString::from(path))
    }
}

fn path_which(name: &str) -> Option<OsString> {
    path_which_with_filter(name, |_| true)
}

fn path_which_excluding_shims(name: &str) -> Option<OsString> {
    let shim_dir = default_shim_dir();
    path_which_with_filter(name, |candidate| {
        candidate.parent() != Some(shim_dir.as_path())
    })
}

fn path_which_with_filter(name: &str, keep: impl Fn(&Path) -> bool) -> Option<OsString> {
    let paths = env::var_os("PATH")?;
    for dir in env::split_paths(&paths) {
        let candidate = dir.join(name);
        if keep(&candidate) && is_executable_file(&candidate) {
            return Some(candidate.into_os_string());
        }
    }
    None
}

fn command_index(args: &[OsString]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "--" {
            return None;
        }
        if arg.starts_with("--") {
            let name = long_flag_name(&arg);
            if global_flag_takes_value(name) && !arg.contains('=') {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if arg.starts_with('-') && arg.len() > 1 {
            if short_global_flag_takes_value(&arg) {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        return Some(i);
    }
    None
}

fn install_has_packages(args: &[OsString]) -> bool {
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "--" {
            return i + 1 < args.len();
        }
        if arg.starts_with("--") {
            let name = long_flag_name(&arg);
            if install_flag_takes_value(name) && !arg.contains('=') {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if arg.starts_with('-') && arg.len() > 1 {
            if short_install_flag_takes_value(&arg) {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        return true;
    }
    false
}

fn translate_npm_install_package_args(args: &[OsString]) -> Vec<OsString> {
    args.iter()
        .filter_map(|arg| {
            let s = arg.to_string_lossy();
            match s.as_ref() {
                "--save" | "--save-prod" => None,
                _ => Some(arg.clone()),
            }
        })
        .collect()
}

fn move_global_markers(args: &[OsString]) -> (Vec<OsString>, Vec<OsString>) {
    let mut prefix = Vec::new();
    let mut moved = Vec::new();
    for arg in args {
        match arg.to_string_lossy().as_ref() {
            "-g" | "--global" => moved.push(arg.clone()),
            _ => prefix.push(arg.clone()),
        }
    }
    (prefix, moved)
}

fn normalize_npm_command(command: &str) -> &'static str {
    match command {
        "i" | "in" => "install",
        "un" | "uni" | "uninstall" => "remove",
        "run-script" => "run",
        "up" | "upgrade" => "update",
        other => known_aube_name(other),
    }
}

fn known_aube_name(command: &str) -> &'static str {
    match command {
        "add" => "add",
        "audit" => "audit",
        "bin" => "bin",
        "cache" => "cache",
        "ci" => "ci",
        "clean" => "clean",
        "config" => "config",
        "create" => "create",
        "dedupe" => "dedupe",
        "deprecate" => "deprecate",
        "dist-tag" | "dist-tags" => "dist-tag",
        "dlx" => "dlx",
        "exec" | "x" => "exec",
        "explain" => "why",
        "help" => "help",
        "info" | "show" | "view" => "view",
        "init" => "init",
        "install" => "install",
        "licenses" => "licenses",
        "link" => "link",
        "list" | "ls" => "list",
        "login" | "adduser" => "login",
        "logout" => "logout",
        "outdated" => "outdated",
        "pack" => "pack",
        "prune" => "prune",
        "publish" => "publish",
        "rebuild" => "rebuild",
        "remove" | "rm" => "remove",
        "restart" => "restart",
        "root" => "root",
        "run" => "run",
        "start" => "start",
        "stop" => "stop",
        "test" | "t" => "test",
        "unpublish" => "unpublish",
        "update" => "update",
        "version" => "version",
        "why" => "why",
        _ => unreachable!("known_aube_name called with unknown command"),
    }
}

fn known_npm_command(command: &str) -> bool {
    matches!(
        command,
        "add"
            | "adduser"
            | "audit"
            | "bin"
            | "cache"
            | "ci"
            | "clean"
            | "config"
            | "create"
            | "dedupe"
            | "deprecate"
            | "dist-tag"
            | "dist-tags"
            | "dlx"
            | "exec"
            | "explain"
            | "help"
            | "i"
            | "in"
            | "info"
            | "init"
            | "install"
            | "licenses"
            | "link"
            | "list"
            | "login"
            | "logout"
            | "ls"
            | "outdated"
            | "pack"
            | "prune"
            | "publish"
            | "rebuild"
            | "remove"
            | "restart"
            | "rm"
            | "root"
            | "run"
            | "run-script"
            | "show"
            | "start"
            | "stop"
            | "t"
            | "test"
            | "un"
            | "uni"
            | "uninstall"
            | "unpublish"
            | "up"
            | "update"
            | "upgrade"
            | "version"
            | "view"
            | "why"
            | "x"
    ) || npm_only_command(command)
}

fn npm_only_command(command: &str) -> bool {
    matches!(
        command,
        "owner" | "pkg" | "search" | "set-script" | "token" | "whoami"
    )
}

fn global_flag_takes_value(name: &str) -> bool {
    matches!(
        name,
        "cache" | "color" | "loglevel" | "prefix" | "registry" | "userconfig"
    )
}

fn install_flag_takes_value(name: &str) -> bool {
    matches!(
        name,
        "cache"
            | "cpu"
            | "include"
            | "install-strategy"
            | "libc"
            | "loglevel"
            | "omit"
            | "os"
            | "prefix"
            | "registry"
            | "save-prefix"
            | "tag"
            | "userconfig"
            | "workspace"
    )
}

fn short_global_flag_takes_value(arg: &str) -> bool {
    matches!(arg, "-C")
}

fn short_install_flag_takes_value(arg: &str) -> bool {
    matches!(arg, "-C" | "-w")
}

fn long_flag_name(arg: &str) -> &str {
    arg.trim_start_matches("--")
        .split_once('=')
        .map(|(name, _)| name)
        .unwrap_or_else(|| arg.trim_start_matches("--"))
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }

    1
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

fn is_executable_file(path: &Path) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn npm_install_without_packages_uses_aube_install() {
        let plan = plan_for(ShimTool::Npm, &os(&["install"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn npm_install_with_packages_becomes_aube_add() {
        let plan = plan_for(ShimTool::Npm, &os(&["i", "-D", "vitest"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["add", "-D", "vitest"]);
    }

    #[test]
    fn npm_install_with_global_prefix_keeps_prefix_before_add() {
        let plan = plan_for(
            ShimTool::Npm,
            &os(&["--prefix", "packages/app", "install", "react"]),
        );

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(
            strings(&plan.args),
            vec!["--prefix", "packages/app", "add", "react"]
        );
    }

    #[test]
    fn npm_global_install_with_package_becomes_global_add() {
        let plan = plan_for(ShimTool::Npm, &os(&["-g", "install", "cowsay"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["add", "-g", "cowsay"]);
    }

    #[test]
    fn npm_install_with_workspace_value_does_not_treat_value_as_package() {
        let plan = plan_for(ShimTool::Npm, &os(&["install", "--workspace", "app"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["install", "--workspace", "app"]);
    }

    #[test]
    fn npm_run_script_uses_aube_run() {
        let plan = plan_for(ShimTool::Npm, &os(&["run-script", "build"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["run", "build"]);
    }

    #[test]
    fn npm_only_command_uses_real_npm() {
        let plan = plan_for(ShimTool::Npm, &os(&["pkg", "get", "name"]));

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["pkg", "get", "name"]);
    }

    #[test]
    fn unknown_npm_command_uses_real_npm() {
        let plan = plan_for(ShimTool::Npm, &os(&["doctor"]));

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["doctor"]);
    }

    #[test]
    fn pnpm_passes_through_to_aube() {
        let plan = plan_for(ShimTool::Pnpm, &os(&["install", "--frozen-lockfile"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["install", "--frozen-lockfile"]);
    }

    #[test]
    fn shell_init_supports_fish() {
        let init = shell_init(
            Shell::Fish,
            Path::new("/home/me/.local/share/aubeshim/shims"),
        );

        assert_eq!(
            init,
            "fish_add_path --prepend '/home/me/.local/share/aubeshim/shims'\n"
        );
    }

    #[test]
    fn shell_init_supports_sh() {
        let init = shell_init(Shell::Sh, Path::new("/home/me/.local/share/aubeshim/shims"));

        assert!(init.contains("AUBESHIM_SHIM_DIR="));
        assert!(init.contains("export PATH"));
    }

    #[test]
    fn shell_init_supports_bash() {
        let init = shell_init(
            Shell::Bash,
            Path::new("/home/me/.local/share/aubeshim/shims"),
        );

        assert!(init.contains("PATH=\"${PATH//:$_aubeshim_shim_dir:/:}\""));
        assert!(init.contains("export PATH=\"$_aubeshim_shim_dir:$PATH\""));
    }

    #[cfg(unix)]
    #[test]
    fn install_and_uninstall_shims() {
        let dir = tempfile::tempdir().unwrap();
        let installed = install_shims(dir.path(), false).unwrap();

        assert_eq!(installed.len(), 2);
        assert!(dir.path().join("npm").is_symlink());
        assert!(dir.path().join("pnpm").is_symlink());

        let removed = uninstall_shims(dir.path()).unwrap();
        assert_eq!(removed.len(), 2);
        assert!(!dir.path().join("npm").exists());
        assert!(!dir.path().join("pnpm").exists());
    }
}
