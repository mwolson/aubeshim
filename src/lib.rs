use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::cmp::Ordering;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitStatus};

const SHIM_NAMES: &[&str] = &["bun", "npm", "pnpm", "yarn"];
const MIN_MISE_VERSION: &str = "2026.5.6";

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
    Activate {
        /// Shell syntax to emit
        shell: Shell,
        /// Shim directory to put on PATH
        #[arg(long, value_name = "DIR")]
        shim_dir: Option<PathBuf>,
    },
    /// Create package-manager shims that point at this executable
    Install {
        /// Replace existing shim files
        #[arg(long)]
        force: bool,
        /// Directory where package-manager shims should be installed
        #[arg(long, value_name = "DIR")]
        shim_dir: Option<PathBuf>,
    },
    /// Remove bun, npm, pnpm, and yarn shims
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
    Bun,
    Npm,
    Pnpm,
    Yarn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub target: Target,
    pub args: Vec<OsString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Aube,
    Mise,
    RealBun,
    RealNpm,
    RealPnpm,
    RealYarn,
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
        "npm" => Invocation::Shim(ShimTool::Npm),
        "pnpm" => Invocation::Shim(ShimTool::Pnpm),
        "yarn" => Invocation::Shim(ShimTool::Yarn),
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
        ShimTool::Bun => plan_bun(args),
        ShimTool::Npm => plan_npm(args),
        ShimTool::Pnpm => plan_pnpm(args),
        ShimTool::Yarn => plan_yarn(args),
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
        Shell::Fish => format!(
            "set -l _aubeshim_shim_dir {dir}\nset -gx PATH (string match --invert -- $_aubeshim_shim_dir $PATH)\nfish_add_path --path --prepend $_aubeshim_shim_dir\nset -e _aubeshim_shim_dir\n"
        ),
        Shell::Sh => format!(
            "AUBESHIM_SHIM_DIR=${{AUBESHIM_SHIM_DIR:-{dir}}}\n_aubeshim_old_path=$PATH\nPATH=$AUBESHIM_SHIM_DIR\nIFS=:\nfor _aubeshim_path_entry in $_aubeshim_old_path; do\n    if [ \"$_aubeshim_path_entry\" != \"$AUBESHIM_SHIM_DIR\" ]; then\n        PATH=\"$PATH:$_aubeshim_path_entry\"\n    fi\ndone\nunset IFS _aubeshim_old_path _aubeshim_path_entry\nexport PATH\n"
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

    if npm_json_metadata_command(&command) && has_json_marker(args) {
        return Plan {
            target: Target::RealNpm,
            args: args.to_vec(),
        };
    }

    if command == "outdated" && has_global_marker(args) {
        return plan_mise_global_outdated(rest);
    }

    if npm_global_package_operation(&command) && has_global_marker(args) {
        return Plan {
            target: Target::RealNpm,
            args: args.to_vec(),
        };
    }

    if matches!(command.as_str(), "install" | "i" | "in") && install_has_packages(rest) {
        let mut out = Vec::with_capacity(args.len());
        out.extend_from_slice(prefix);
        out.push(OsString::from("add"));
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

fn plan_pnpm(args: &[OsString]) -> Plan {
    if let Some(command_idx) = command_index(args) {
        let command = args[command_idx].to_string_lossy().to_ascii_lowercase();
        if command == "outdated" && has_global_marker(args) {
            return plan_mise_global_outdated(&args[command_idx + 1..]);
        }
        if pnpm_global_package_operation(&command) && has_global_marker(args) {
            return Plan {
                target: Target::RealPnpm,
                args: args.to_vec(),
            };
        }
    }

    Plan {
        target: Target::Aube,
        args: args.to_vec(),
    }
}

fn plan_bun(args: &[OsString]) -> Plan {
    let Some(command_idx) = command_index(args) else {
        return Plan {
            target: Target::RealBun,
            args: args.to_vec(),
        };
    };
    let command = args[command_idx].to_string_lossy().to_ascii_lowercase();
    let prefix = &args[..command_idx];
    let rest = &args[command_idx + 1..];

    let Some(command) = normalize_bun_command(&command) else {
        return Plan {
            target: Target::RealBun,
            args: args.to_vec(),
        };
    };

    if command == "outdated" && has_global_marker(args) {
        return plan_mise_global_outdated(rest);
    }

    if bun_global_package_operation(command) && has_global_marker(args) {
        return Plan {
            target: Target::RealBun,
            args: args.to_vec(),
        };
    }

    let mut out = Vec::with_capacity(args.len());
    out.extend_from_slice(prefix);
    out.push(OsString::from(command));
    out.extend_from_slice(rest);
    Plan {
        target: Target::Aube,
        args: out,
    }
}

fn plan_yarn(args: &[OsString]) -> Plan {
    let Some(command_idx) = command_index(args) else {
        if !args.is_empty() {
            return Plan {
                target: Target::Aube,
                args: args.to_vec(),
            };
        }
        return Plan {
            target: Target::Aube,
            args: vec![OsString::from("install")],
        };
    };
    let command = args[command_idx].to_string_lossy().to_ascii_lowercase();
    let prefix = &args[..command_idx];
    let rest = &args[command_idx + 1..];

    if yarn_only_command(&command) {
        return Plan {
            target: Target::RealYarn,
            args: args.to_vec(),
        };
    }

    if command == "outdated" && has_global_marker(args) {
        return plan_mise_global_outdated(rest);
    }

    if yarn_global_package_operation(&command) && has_global_marker(args) {
        return Plan {
            target: Target::RealYarn,
            args: args.to_vec(),
        };
    }

    let mut out = Vec::with_capacity(args.len());
    out.extend_from_slice(prefix);
    out.push(OsString::from(normalize_yarn_command(&command)));
    out.extend_from_slice(rest);
    Plan {
        target: Target::Aube,
        args: out,
    }
}

fn plan_mise_global_outdated(args: &[OsString]) -> Plan {
    let mut out = vec![
        OsString::from("outdated"),
        OsString::from("--bump"),
        OsString::from("-C"),
        home_dir().into_os_string(),
    ];
    out.extend(translate_global_outdated_args(args));
    Plan {
        target: Target::Mise,
        args: out,
    }
}

fn run_plan(plan: Plan) -> Result<ExitStatus> {
    let program = resolve_target(plan.target)?;
    let mut cmd = ProcessCommand::new(&program);
    cmd.args(&plan.args);

    if matches!(plan.target, Target::Aube) && env::var_os("AUBE_NPM_PATH").is_none() {
        if let Some(npm) = resolve_real_npm()? {
            cmd.env("AUBE_NPM_PATH", npm);
        }
    }

    cmd.status()
        .with_context(|| format!("failed to run {}", PathBuf::from(program).display()))
}

fn resolve_target(target: Target) -> Result<OsString> {
    match target {
        Target::Aube => resolve_aube()?.ok_or_else(|| missing_tool_error("aube", "AUBESHIM_AUBE")),
        Target::Mise => resolve_mise()?.ok_or_else(missing_mise_error),
        Target::RealBun => {
            resolve_real_bun()?.ok_or_else(|| missing_tool_error("real bun", "AUBESHIM_REAL_BUN"))
        }
        Target::RealNpm => {
            resolve_real_npm()?.ok_or_else(|| missing_tool_error("real npm", "AUBESHIM_REAL_NPM"))
        }
        Target::RealPnpm => resolve_real_pnpm()?
            .ok_or_else(|| missing_tool_error("real pnpm", "AUBESHIM_REAL_PNPM")),
        Target::RealYarn => resolve_real_yarn()?
            .ok_or_else(|| missing_tool_error("real yarn", "AUBESHIM_REAL_YARN")),
    }
}

fn resolve_mise() -> Result<Option<OsString>> {
    let Some(mise) = path_which("mise") else {
        return Ok(None);
    };
    ensure_supported_mise(&mise)?;
    Ok(Some(mise))
}

fn resolve_aube() -> Result<Option<OsString>> {
    resolve_tool("aube", "AUBESHIM_AUBE", path_which)
}

fn resolve_real_bun() -> Result<Option<OsString>> {
    resolve_tool("bun", "AUBESHIM_REAL_BUN", path_which_excluding_shims)
}

fn resolve_real_npm() -> Result<Option<OsString>> {
    resolve_tool("npm", "AUBESHIM_REAL_NPM", path_which_excluding_shims)
}

fn resolve_real_pnpm() -> Result<Option<OsString>> {
    resolve_tool("pnpm", "AUBESHIM_REAL_PNPM", path_which_excluding_shims)
}

fn resolve_real_yarn() -> Result<Option<OsString>> {
    resolve_tool("yarn", "AUBESHIM_REAL_YARN", path_which_excluding_shims)
}

fn resolve_tool(
    tool: &str,
    env_var: &str,
    path_lookup: fn(&str) -> Option<OsString>,
) -> Result<Option<OsString>> {
    if let Some(path) = env::var_os(env_var) {
        return Ok(Some(path));
    }

    if let Some(path) = mise_which(tool)? {
        return Ok(Some(path));
    }

    Ok(path_lookup(tool))
}

fn mise_which(tool: &str) -> Result<Option<OsString>> {
    let Some(mise) = path_which("mise") else {
        return Ok(None);
    };
    ensure_supported_mise(&mise)?;

    let output = ProcessCommand::new(&mise)
        .arg("which")
        .arg(tool)
        .output()
        .with_context(|| format!("failed to run {}", PathBuf::from(&mise).display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    let path = String::from_utf8(output.stdout).context("mise which output was not UTF-8")?;
    let path = path.trim();
    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(OsString::from(path)))
    }
}

fn ensure_supported_mise(mise: &OsStr) -> Result<()> {
    let output = ProcessCommand::new(mise)
        .arg("--version")
        .output()
        .with_context(|| format!("failed to run {}", PathBuf::from(mise).display()))?;
    if !output.status.success() {
        bail!(
            "failed to check mise version with {}",
            PathBuf::from(mise).display()
        );
    }

    let stdout = String::from_utf8(output.stdout).context("mise --version output was not UTF-8")?;
    let version = mise_version_from_output(&stdout)
        .ok_or_else(|| anyhow!("could not parse mise version from `{}`", stdout.trim()))?;
    if compare_dotted_versions(version, MIN_MISE_VERSION) == Ordering::Less {
        return Err(unsupported_mise_error(version));
    }

    Ok(())
}

fn mise_version_from_output(output: &str) -> Option<&str> {
    output
        .split_whitespace()
        .find(|token| token.chars().any(|ch| ch.is_ascii_digit()))
}

fn unsupported_mise_error(version: &str) -> anyhow::Error {
    anyhow!(
        "mise {version} is too old for aubeshim; install mise >= {MIN_MISE_VERSION}. Arch Linux's mise package may lag behind the version needed for aube support."
    )
}

fn compare_dotted_versions(left: &str, right: &str) -> Ordering {
    let mut left_parts = left.split('.').map(parse_version_part);
    let mut right_parts = right.split('.').map(parse_version_part);

    loop {
        match (left_parts.next(), right_parts.next()) {
            (None, None) => return Ordering::Equal,
            (left, right) => {
                let ordering = left.unwrap_or(0).cmp(&right.unwrap_or(0));
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
        }
    }
}

fn parse_version_part(part: &str) -> u32 {
    part.chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

fn missing_tool_error(tool: &str, env_var: &str) -> anyhow::Error {
    let mise_hint = if command_on_path("mise") {
        "aubeshim also tried `mise which`; make sure the tool is installed with mise"
    } else {
        "mise is not on PATH; install mise first if you expect aubeshim to find tools through mise"
    };
    anyhow!("could not find {tool}; set {env_var} to an absolute path, install it another way, or install it with mise. {mise_hint}")
}

fn missing_mise_error() -> anyhow::Error {
    anyhow!(
        "could not find mise; install mise >= {MIN_MISE_VERSION} to use aubeshim global outdated support"
    )
}

fn path_which(name: &str) -> Option<OsString> {
    path_which_with_filter(name, |_| true)
}

fn command_on_path(name: &str) -> bool {
    path_which(name).is_some()
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

fn has_global_marker(args: &[OsString]) -> bool {
    args.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        arg == "-g" || arg == "--global" || arg.starts_with("--global=")
    })
}

fn has_json_marker(args: &[OsString]) -> bool {
    args.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        arg == "--json" || arg.starts_with("--json=")
    })
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

fn translate_global_outdated_args(args: &[OsString]) -> Vec<OsString> {
    args.iter()
        .filter_map(|arg| {
            let s = arg.to_string_lossy();
            match s.as_ref() {
                "-g" | "--global" => None,
                "--json" => Some(arg.clone()),
                value if value.starts_with("--global=") => None,
                value if value.starts_with('-') => None,
                package => Some(OsString::from(format!("npm:{package}"))),
            }
        })
        .collect()
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

fn npm_global_package_operation(command: &str) -> bool {
    matches!(
        command,
        "add" | "i" | "in" | "install" | "remove" | "rm" | "un" | "uni" | "uninstall"
    )
}

fn npm_json_metadata_command(command: &str) -> bool {
    matches!(command, "info" | "show" | "view")
}

fn pnpm_global_package_operation(command: &str) -> bool {
    matches!(
        command,
        "add" | "i" | "install" | "remove" | "rm" | "uninstall"
    )
}

fn bun_global_package_operation(command: &str) -> bool {
    matches!(command, "add" | "install" | "remove")
}

fn yarn_global_package_operation(command: &str) -> bool {
    matches!(
        command,
        "add" | "install" | "remove" | "rm" | "upgrade" | "up"
    )
}

fn normalize_yarn_command(command: &str) -> String {
    match command {
        "info" => "view".to_owned(),
        "upgrade" | "up" => "update".to_owned(),
        other => known_yarn_name(other).unwrap_or(other).to_owned(),
    }
}

fn normalize_bun_command(command: &str) -> Option<&'static str> {
    match command {
        "add" => Some("add"),
        "i" | "install" => Some("install"),
        "link" => Some("link"),
        "outdated" => Some("outdated"),
        "publish" => Some("publish"),
        "remove" | "rm" => Some("remove"),
        "run" => Some("run"),
        "unlink" => Some("unlink"),
        "update" | "upgrade" => Some("update"),
        "x" => Some("dlx"),
        _ => None,
    }
}

fn known_yarn_name(command: &str) -> Option<&'static str> {
    match command {
        "add" => Some("add"),
        "bin" => Some("bin"),
        "cache" => Some("cache"),
        "config" => Some("config"),
        "create" => Some("create"),
        "dedupe" => Some("dedupe"),
        "dlx" => Some("dlx"),
        "exec" => Some("exec"),
        "help" => Some("help"),
        "init" => Some("init"),
        "install" => Some("install"),
        "link" => Some("link"),
        "login" => Some("login"),
        "logout" => Some("logout"),
        "outdated" => Some("outdated"),
        "pack" => Some("pack"),
        "publish" => Some("publish"),
        "remove" | "rm" => Some("remove"),
        "run" => Some("run"),
        "start" => Some("start"),
        "test" => Some("test"),
        "unlink" => Some("unlink"),
        "version" => Some("version"),
        "why" => Some("why"),
        _ => None,
    }
}

fn yarn_only_command(command: &str) -> bool {
    matches!(
        command,
        "constraints" | "global" | "node" | "npm" | "plugin" | "set" | "workspaces"
    )
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

    fn mise_global_outdated_args(extra: &[&str]) -> Vec<String> {
        let mut args = vec![
            "outdated".to_owned(),
            "--bump".to_owned(),
            "-C".to_owned(),
            home_dir().to_string_lossy().into_owned(),
        ];
        args.extend(extra.iter().map(|arg| (*arg).to_owned()));
        args
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
    fn npm_global_install_with_package_uses_real_npm() {
        let plan = plan_for(ShimTool::Npm, &os(&["-g", "install", "cowsay"]));

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["-g", "install", "cowsay"]);
    }

    #[test]
    fn npm_global_remove_uses_real_npm() {
        let remove = plan_for(ShimTool::Npm, &os(&["remove", "--global", "cowsay"]));

        assert_eq!(remove.target, Target::RealNpm);
        assert_eq!(strings(&remove.args), vec!["remove", "--global", "cowsay"]);
    }

    #[test]
    fn npm_global_outdated_uses_mise() {
        let plan = plan_for(
            ShimTool::Npm,
            &os(&["outdated", "--global", "@biomejs/biome"]),
        );

        assert_eq!(plan.target, Target::Mise);
        assert_eq!(
            strings(&plan.args),
            mise_global_outdated_args(&["npm:@biomejs/biome"])
        );
    }

    #[test]
    fn npm_json_metadata_commands_use_real_npm() {
        for args in [
            &["view", "prettier", "dist-tags", "--json"][..],
            &["show", "typescript", "version", "--json=true"][..],
            &["info", "eslint", "--json"][..],
        ] {
            let plan = plan_for(ShimTool::Npm, &os(args));

            assert_eq!(plan.target, Target::RealNpm);
            assert_eq!(strings(&plan.args), args);
        }
    }

    #[test]
    fn npm_metadata_without_json_still_uses_aube_view() {
        let plan = plan_for(ShimTool::Npm, &os(&["show", "typescript", "version"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["view", "typescript", "version"]);
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
    fn pnpm_global_package_operations_use_real_pnpm() {
        for args in [
            &["add", "-g", "cowsay"][..],
            &["install", "--global", "typescript"][..],
            &["remove", "-g", "cowsay"][..],
        ] {
            let plan = plan_for(ShimTool::Pnpm, &os(args));

            assert_eq!(plan.target, Target::RealPnpm);
            assert_eq!(strings(&plan.args), args);
        }
    }

    #[test]
    fn pnpm_global_outdated_uses_mise() {
        let plan = plan_for(ShimTool::Pnpm, &os(&["outdated", "-g"]));

        assert_eq!(plan.target, Target::Mise);
        assert_eq!(strings(&plan.args), mise_global_outdated_args(&[]));
    }

    #[test]
    fn bun_install_uses_aube_install() {
        let plan = plan_for(ShimTool::Bun, &os(&["install", "--frozen-lockfile"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["install", "--frozen-lockfile"]);
    }

    #[test]
    fn bun_run_uses_aube_run() {
        let plan = plan_for(ShimTool::Bun, &os(&["run", "dev"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["run", "dev"]);
    }

    #[test]
    fn bun_global_package_operations_use_real_bun() {
        for args in [
            &["add", "-g", "cowsay"][..],
            &["install", "--global", "typescript"][..],
            &["remove", "-g", "cowsay"][..],
        ] {
            let plan = plan_for(ShimTool::Bun, &os(args));

            assert_eq!(plan.target, Target::RealBun);
            assert_eq!(strings(&plan.args), args);
        }
    }

    #[test]
    fn bun_global_outdated_uses_mise() {
        let plan = plan_for(ShimTool::Bun, &os(&["outdated", "-g", "--json"]));

        assert_eq!(plan.target, Target::Mise);
        assert_eq!(strings(&plan.args), mise_global_outdated_args(&["--json"]));
    }

    #[test]
    fn bun_runtime_command_uses_real_bun() {
        let plan = plan_for(ShimTool::Bun, &os(&["test", "src/app.test.ts"]));

        assert_eq!(plan.target, Target::RealBun);
        assert_eq!(strings(&plan.args), vec!["test", "src/app.test.ts"]);
    }

    #[test]
    fn yarn_without_args_installs() {
        let plan = plan_for(ShimTool::Yarn, &os(&[]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn yarn_version_flag_passes_through() {
        let plan = plan_for(ShimTool::Yarn, &os(&["--version"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["--version"]);
    }

    #[test]
    fn yarn_run_style_script_passes_to_aube_external_script() {
        let plan = plan_for(ShimTool::Yarn, &os(&["dev", "--host"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["dev", "--host"]);
    }

    #[test]
    fn yarn_global_package_operations_use_real_yarn() {
        for args in [
            &["add", "-g", "cowsay"][..],
            &["install", "--global", "typescript"][..],
            &["remove", "-g", "cowsay"][..],
        ] {
            let plan = plan_for(ShimTool::Yarn, &os(args));

            assert_eq!(plan.target, Target::RealYarn);
            assert_eq!(strings(&plan.args), args);
        }
    }

    #[test]
    fn yarn_global_outdated_uses_mise() {
        let plan = plan_for(
            ShimTool::Yarn,
            &os(&["outdated", "--global=true", "oxlint"]),
        );

        assert_eq!(plan.target, Target::Mise);
        assert_eq!(
            strings(&plan.args),
            mise_global_outdated_args(&["npm:oxlint"])
        );
    }

    #[test]
    fn yarn_only_command_uses_real_yarn() {
        let plan = plan_for(ShimTool::Yarn, &os(&["plugin", "list"]));

        assert_eq!(plan.target, Target::RealYarn);
        assert_eq!(strings(&plan.args), vec!["plugin", "list"]);
    }

    #[test]
    fn missing_tool_error_mentions_mise_and_override() {
        let message = missing_tool_error("real npm", "AUBESHIM_REAL_NPM").to_string();

        assert!(message.contains("could not find real npm"));
        assert!(message.contains("AUBESHIM_REAL_NPM"));
        assert!(message.contains("mise"));
    }

    #[test]
    fn parses_mise_version_prefix() {
        assert_eq!(
            mise_version_from_output("2026.5.6 linux-x64 (2026-05-11)"),
            Some("2026.5.6")
        );
        assert_eq!(
            mise_version_from_output("mise 2026.5.6 linux-x64"),
            Some("2026.5.6")
        );
    }

    #[test]
    fn compares_dotted_versions_numerically() {
        assert_eq!(
            compare_dotted_versions("2026.5.6", "2026.5.6"),
            Ordering::Equal
        );
        assert_eq!(
            compare_dotted_versions("2026.5.10", "2026.5.6"),
            Ordering::Greater
        );
        assert_eq!(
            compare_dotted_versions("2026.5.5", "2026.5.6"),
            Ordering::Less
        );
    }

    #[test]
    fn unsupported_mise_error_mentions_arch_and_minimum() {
        let message = unsupported_mise_error("2026.5.5").to_string();

        assert!(message.contains("mise 2026.5.5 is too old"));
        assert!(message.contains("mise >= 2026.5.6"));
        assert!(message.contains("Arch Linux"));
    }

    #[test]
    fn shell_init_supports_fish() {
        let init = shell_init(
            Shell::Fish,
            Path::new("/home/me/.local/share/aubeshim/shims"),
        );

        assert!(init.contains("string match --invert"));
        assert!(init.contains("fish_add_path --path --prepend $_aubeshim_shim_dir"));
    }

    #[test]
    fn fish_activation_removes_existing_shim_entries_before_prepending() {
        let dir = tempfile::tempdir().unwrap();
        let shim_dir = dir.path().to_string_lossy();
        let path = format!("/bin:{shim_dir}:/usr/bin:{shim_dir}:/sbin");
        let output = run_shell_activation("fish", Shell::Fish, dir.path(), &path);

        assert_eq!(output, format!("{shim_dir}:/bin:/usr/bin:/sbin"));
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

    #[test]
    fn bash_activation_removes_existing_shim_entries_before_prepending() {
        let dir = Path::new("/tmp/aubeshim-test");
        let output = run_shell_activation(
            "bash",
            Shell::Bash,
            dir,
            "/bin:/tmp/aubeshim-test:/usr/bin:/tmp/aubeshim-test:/sbin",
        );

        assert_eq!(output, "/tmp/aubeshim-test:/bin:/usr/bin:/sbin");
    }

    #[test]
    fn sh_activation_removes_existing_shim_entries_before_prepending() {
        let dir = Path::new("/tmp/aubeshim-test");
        let output = run_shell_activation(
            "sh",
            Shell::Sh,
            dir,
            "/bin:/tmp/aubeshim-test:/usr/bin:/tmp/aubeshim-test:/sbin",
        );

        assert_eq!(output, "/tmp/aubeshim-test:/bin:/usr/bin:/sbin");
    }

    #[test]
    fn zsh_activation_removes_existing_shim_entries_before_prepending() {
        let dir = Path::new("/tmp/aubeshim-test");
        let output = run_shell_activation(
            "zsh",
            Shell::Zsh,
            dir,
            "/bin:/tmp/aubeshim-test:/usr/bin:/tmp/aubeshim-test:/sbin",
        );

        assert_eq!(output, "/tmp/aubeshim-test:/bin:/usr/bin:/sbin");
    }

    #[cfg(unix)]
    #[test]
    fn install_and_uninstall_shims() {
        let dir = tempfile::tempdir().unwrap();
        let installed = install_shims(dir.path(), false).unwrap();

        assert_eq!(installed.len(), 4);
        assert!(dir.path().join("bun").is_symlink());
        assert!(dir.path().join("npm").is_symlink());
        assert!(dir.path().join("pnpm").is_symlink());
        assert!(dir.path().join("yarn").is_symlink());

        let removed = uninstall_shims(dir.path()).unwrap();
        assert_eq!(removed.len(), 4);
        assert!(!dir.path().join("bun").exists());
        assert!(!dir.path().join("npm").exists());
        assert!(!dir.path().join("pnpm").exists());
        assert!(!dir.path().join("yarn").exists());
    }

    fn run_shell_activation(shell: &str, init_shell: Shell, dir: &Path, path: &str) -> String {
        let script = format!(
            "{}\nprintf '%s\\n' \"$PATH\"\n",
            shell_init(init_shell, dir)
        );
        let zdotdir = tempfile::tempdir().unwrap();
        let mut cmd = std::process::Command::new(shell);
        if shell == "fish" {
            cmd.arg("--no-config");
        }
        cmd.arg("-c").arg(script).env("PATH", path);
        if shell == "zsh" {
            cmd.env("ZDOTDIR", zdotdir.path());
        }
        let output = cmd.output().unwrap();

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end()
            .to_owned()
    }
}
