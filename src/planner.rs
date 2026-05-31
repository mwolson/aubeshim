use crate::config::{should_shim, Config};
use crate::home_dir;
use crate::shims::ShimTool;
use anyhow::Result;
use std::ffi::{OsStr, OsString};
use std::path::Path;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlobalPackageAction {
    Use,
    Unuse,
}
pub fn plan_for(tool: ShimTool, args: &[OsString]) -> Plan {
    match tool {
        ShimTool::Bun => plan_bun(args),
        ShimTool::Npm => plan_npm(args),
        ShimTool::Pnpm => plan_pnpm(args),
        ShimTool::Yarn => plan_yarn(args),
    }
}

pub fn plan_for_config(
    tool: ShimTool,
    args: &[OsString],
    config: &Config,
    cwd: &Path,
) -> Result<Plan> {
    if should_shim(config, cwd)? {
        return Ok(plan_for(tool, args));
    }

    Ok(Plan {
        target: real_target_for(tool),
        args: args.to_vec(),
    })
}

fn real_target_for(tool: ShimTool) -> Target {
    match tool {
        ShimTool::Bun => Target::RealBun,
        ShimTool::Npm => Target::RealNpm,
        ShimTool::Pnpm => Target::RealPnpm,
        ShimTool::Yarn => Target::RealYarn,
    }
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

    if matches!(command.as_str(), "list" | "ls") && npm_list_requires_real_npm(args) {
        return Plan {
            target: Target::RealNpm,
            args: args.to_vec(),
        };
    }

    if command == "outdated" && has_global_marker(args) {
        return plan_mise_global_outdated(rest);
    }

    if has_global_marker(args) {
        if let Some(action) = npm_global_package_action(&command) {
            return plan_mise_global_package_action(action, rest).unwrap_or_else(|| Plan {
                target: Target::RealNpm,
                args: args.to_vec(),
            });
        }
    }

    if matches!(command.as_str(), "install" | "i" | "in") && install_has_packages(rest) {
        let Some(translated_rest) = translate_npm_install_package_args(rest) else {
            return Plan {
                target: Target::RealNpm,
                args: args.to_vec(),
            };
        };
        let mut out = Vec::with_capacity(args.len());
        out.extend_from_slice(prefix);
        out.push(OsString::from("add"));
        out.extend(translated_rest);
        return Plan {
            target: Target::Aube,
            args: out,
        };
    }

    let mut out = Vec::with_capacity(args.len());
    out.extend_from_slice(prefix);
    out.push(OsString::from(normalize_npm_command(&command)));
    if matches!(command.as_str(), "ci" | "install" | "i" | "in") {
        let Some(translated) = translate_npm_project_install_args(rest) else {
            return Plan {
                target: Target::RealNpm,
                args: args.to_vec(),
            };
        };
        out.extend(translated);
    } else {
        out.extend_from_slice(rest);
    }
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
        if has_global_marker(args) {
            if let Some(action) = pnpm_global_package_action(&command) {
                return plan_mise_global_package_action(action, &args[command_idx + 1..])
                    .unwrap_or_else(|| Plan {
                        target: Target::RealPnpm,
                        args: args.to_vec(),
                    });
            }
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

    if command == "run" && bun_run_uses_real_bun(prefix, rest) {
        return Plan {
            target: Target::RealBun,
            args: args.to_vec(),
        };
    }

    if command == "dlx" {
        return plan_bun_dlx(prefix, rest);
    }

    if has_global_marker(args) {
        if let Some(action) = bun_global_package_action(command) {
            return plan_mise_global_package_action(action, rest).unwrap_or_else(|| Plan {
                target: Target::RealBun,
                args: args.to_vec(),
            });
        }
    }

    let mut out = Vec::with_capacity(args.len());
    out.extend_from_slice(prefix);
    out.push(OsString::from(command));
    if command == "install" {
        let Some(translated) = translate_omit_args(rest) else {
            return Plan {
                target: Target::RealBun,
                args: args.to_vec(),
            };
        };
        out.extend(translated);
    } else {
        out.extend_from_slice(rest);
    }
    Plan {
        target: Target::Aube,
        args: out,
    }
}

fn plan_bun_dlx(prefix: &[OsString], rest: &[OsString]) -> Plan {
    let mut no_install = false;
    let translated_prefix = translate_bun_dlx_prefix(prefix, &mut no_install);
    let translated_rest = translate_bun_dlx_rest(rest, &mut no_install);

    let mut out = Vec::with_capacity(translated_prefix.len() + translated_rest.for_dlx.len() + 3);
    out.extend(translated_prefix);
    if no_install {
        out.push(OsString::from("exec"));
        out.push(OsString::from("--no-install"));
        out.extend(prepare_aube_exec_args(&translated_rest.for_exec));
    } else {
        out.push(OsString::from("dlx"));
        out.extend(translated_rest.for_dlx);
    }
    Plan {
        target: Target::Aube,
        args: out,
    }
}

fn translate_bun_dlx_prefix(args: &[OsString], no_install: &mut bool) -> Vec<OsString> {
    args.iter()
        .filter_map(|arg| {
            let value = arg.to_string_lossy();
            if is_bun_dlx_no_install_flag(&value) {
                *no_install = true;
                return None;
            }
            if is_bun_dlx_install_flag(&value) {
                return None;
            }
            Some(arg.clone())
        })
        .collect()
}

struct TranslatedBunDlxRest {
    for_dlx: Vec<OsString>,
    for_exec: Vec<OsString>,
}

fn translate_bun_dlx_rest(args: &[OsString], no_install: &mut bool) -> TranslatedBunDlxRest {
    let mut for_dlx = Vec::with_capacity(args.len());
    let mut for_exec = Vec::with_capacity(args.len());
    let mut i = 0;

    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "--" {
            for_dlx.extend_from_slice(&args[i..]);
            for_exec.extend_from_slice(&args[i..]);
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            for_dlx.extend_from_slice(&args[i..]);
            for_exec.extend_from_slice(&args[i..]);
            break;
        }
        if is_bun_dlx_no_install_flag(&arg) {
            *no_install = true;
            i += 1;
            continue;
        }
        if is_bun_dlx_install_flag(&arg) {
            i += 1;
            continue;
        }
        if arg == "--package" || arg == "-p" {
            for_dlx.push(args[i].clone());
            if let Some(value) = args.get(i + 1) {
                for_dlx.push(value.clone());
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if arg.starts_with("--package=") {
            for_dlx.push(args[i].clone());
            i += 1;
            continue;
        }

        for_dlx.push(args[i].clone());
        for_exec.push(args[i].clone());
        i += 1;
    }

    TranslatedBunDlxRest { for_dlx, for_exec }
}

fn prepare_aube_exec_args(args: &[OsString]) -> Vec<OsString> {
    let mut out = args.to_vec();
    if let Some(command_idx) = command_index(&out) {
        if command_idx + 1 < out.len() {
            out.insert(command_idx + 1, OsString::from("--"));
        }
    }
    out
}

fn is_bun_dlx_no_install_flag(arg: &str) -> bool {
    arg == "--no-install"
}

fn is_bun_dlx_install_flag(arg: &str) -> bool {
    arg == "-i" || arg.starts_with("--install=")
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

    if has_global_marker(args) {
        if let Some(action) = yarn_global_package_action(&command) {
            return plan_mise_global_package_action(action, rest).unwrap_or_else(|| Plan {
                target: Target::RealYarn,
                args: args.to_vec(),
            });
        }
    }

    let mut out = Vec::with_capacity(args.len());
    out.extend_from_slice(prefix);
    out.push(OsString::from(normalize_yarn_command(&command)));
    if command == "install" {
        out.extend(translate_yarn_install_args(rest));
    } else {
        out.extend_from_slice(rest);
    }
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

fn plan_mise_global_package_action(action: GlobalPackageAction, args: &[OsString]) -> Option<Plan> {
    let packages = translate_global_package_args(args);
    if packages.is_empty() {
        return None;
    }

    let mut out = vec![
        OsString::from(match action {
            GlobalPackageAction::Use => "use",
            GlobalPackageAction::Unuse => "unuse",
        }),
        OsString::from("-g"),
    ];
    out.extend(packages);
    Some(Plan {
        target: Target::Mise,
        args: out,
    })
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
        is_global_marker(&arg)
    })
}

fn has_json_marker(args: &[OsString]) -> bool {
    args.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        arg == "--json" || arg.starts_with("--json=")
    })
}

fn translate_npm_install_package_args(args: &[OsString]) -> Option<Vec<OsString>> {
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--" {
            out.extend_from_slice(&args[i..]);
            break;
        }
        if skip_npm_install_layout_arg(args, &mut i)? {
            continue;
        }

        let arg = args[i].to_string_lossy();
        match arg.as_ref() {
            "--save" | "--save-prod" => {}
            _ => out.push(args[i].clone()),
        }
        i += 1;
    }
    Some(out)
}

fn translate_npm_project_install_args(args: &[OsString]) -> Option<Vec<OsString>> {
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--" {
            out.extend_from_slice(&args[i..]);
            break;
        }
        if skip_npm_install_layout_arg(args, &mut i)? {
            continue;
        }

        let arg = args[i].to_string_lossy();
        if arg == "--omit" {
            let value = args.get(i + 1)?.to_string_lossy();
            push_omit_translation(&mut out, &value)?;
            i += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--omit=") {
            push_omit_translation(&mut out, value)?;
            i += 1;
            continue;
        }

        out.push(args[i].clone());
        i += 1;
    }
    Some(out)
}

fn skip_npm_install_layout_arg(args: &[OsString], i: &mut usize) -> Option<bool> {
    let arg = args[*i].to_string_lossy();
    if arg == "--install-strategy" {
        let value = args.get(*i + 1)?.to_string_lossy();
        if value == "hoisted" {
            *i += 2;
            return Some(true);
        }
        return None;
    }
    if let Some(value) = arg.strip_prefix("--install-strategy=") {
        if value == "hoisted" {
            *i += 1;
            return Some(true);
        }
        return None;
    }

    if arg == "--legacy-bundling" || arg == "--global-style" {
        return None;
    }
    if arg == "--no-legacy-bundling" || arg == "--no-global-style" {
        *i += 1;
        return Some(true);
    }

    for name in ["legacy-bundling", "global-style"] {
        if let Some(value) = arg.strip_prefix(&format!("--{name}=")) {
            if value.eq_ignore_ascii_case("false") {
                *i += 1;
                return Some(true);
            }
            return None;
        }
    }

    Some(false)
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

fn translate_global_package_args(args: &[OsString]) -> Vec<OsString> {
    let mut packages = Vec::new();
    let mut i = 0;
    let mut literal = false;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if !literal && arg == "--" {
            literal = true;
            i += 1;
            continue;
        }
        if !literal && is_global_marker(&arg) {
            i += 1;
            continue;
        }
        if !literal && arg.starts_with("--") {
            let name = long_flag_name(&arg);
            if global_package_flag_takes_value(name) && !arg.contains('=') {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if !literal && arg.starts_with('-') && arg.len() > 1 {
            if short_global_package_flag_takes_value(&arg) {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        packages.push(OsString::from(format!("npm:{arg}")));
        i += 1;
    }
    packages
}

fn translate_omit_args(args: &[OsString]) -> Option<Vec<OsString>> {
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "--omit" {
            let value = args.get(i + 1)?.to_string_lossy();
            push_omit_translation(&mut out, &value)?;
            i += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--omit=") {
            push_omit_translation(&mut out, value)?;
            i += 1;
            continue;
        }
        out.push(args[i].clone());
        i += 1;
    }
    Some(out)
}

fn push_omit_translation(out: &mut Vec<OsString>, value: &str) -> Option<()> {
    for item in value.split(',') {
        match item {
            "dev" => out.push(OsString::from("--prod")),
            "optional" => out.push(OsString::from("--no-optional")),
            _ => return None,
        }
    }
    Some(())
}

fn translate_yarn_install_args(args: &[OsString]) -> Vec<OsString> {
    args.iter()
        .map(|arg| {
            if arg == "--ignore-optional" {
                OsString::from("--no-optional")
            } else {
                arg.clone()
            }
        })
        .collect()
}

fn is_global_marker(arg: &str) -> bool {
    arg == "-g" || arg == "--global" || arg.starts_with("--global=")
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

fn npm_global_package_action(command: &str) -> Option<GlobalPackageAction> {
    match command {
        "add" | "i" | "in" | "install" => Some(GlobalPackageAction::Use),
        "remove" | "rm" | "un" | "uni" | "uninstall" => Some(GlobalPackageAction::Unuse),
        _ => None,
    }
}

fn npm_json_metadata_command(command: &str) -> bool {
    matches!(command, "info" | "show" | "view")
}

fn npm_list_requires_real_npm(args: &[OsString]) -> bool {
    args.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        if arg == "-a" {
            return true;
        }
        if !arg.starts_with("--") {
            return false;
        }

        matches!(
            long_flag_name(&arg),
            "all" | "include" | "json" | "long" | "omit" | "parseable"
        )
    })
}

fn pnpm_global_package_action(command: &str) -> Option<GlobalPackageAction> {
    match command {
        "add" | "i" | "install" => Some(GlobalPackageAction::Use),
        "remove" | "rm" | "uninstall" => Some(GlobalPackageAction::Unuse),
        _ => None,
    }
}

fn bun_global_package_action(command: &str) -> Option<GlobalPackageAction> {
    match command {
        "add" | "install" => Some(GlobalPackageAction::Use),
        "remove" => Some(GlobalPackageAction::Unuse),
        _ => None,
    }
}

fn yarn_global_package_action(command: &str) -> Option<GlobalPackageAction> {
    match command {
        "add" | "install" | "upgrade" | "up" => Some(GlobalPackageAction::Use),
        "remove" | "rm" => Some(GlobalPackageAction::Unuse),
        _ => None,
    }
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

fn bun_run_uses_real_bun(prefix: &[OsString], rest: &[OsString]) -> bool {
    if prefix.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        is_bun_runtime_flag(&arg)
    }) {
        return true;
    }

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].to_string_lossy();
        if arg == "--" {
            return false;
        }
        if !arg.starts_with('-') || arg == "-" {
            return looks_like_bun_file_entrypoint(&arg);
        }
        if is_bun_runtime_flag(&arg) {
            return true;
        }
        i += 1;
    }
    false
}

fn is_bun_runtime_flag(arg: &str) -> bool {
    if !arg.starts_with('-') || arg == "-" {
        return false;
    }
    let name = long_flag_name(arg);
    matches!(
        name,
        "bun"
            | "conditions"
            | "config"
            | "console-depth"
            | "cpu-prof"
            | "cpu-prof-dir"
            | "cpu-prof-interval"
            | "cpu-prof-md"
            | "cpu-prof-name"
            | "cwd"
            | "define"
            | "dns-result-order"
            | "drop"
            | "elide-lines"
            | "env-file"
            | "eval"
            | "expose-gc"
            | "extension-order"
            | "feature"
            | "fetch-preconnect"
            | "heap-prof"
            | "heap-prof-dir"
            | "heap-prof-md"
            | "heap-prof-name"
            | "hot"
            | "if-present"
            | "import"
            | "inspect"
            | "inspect-brk"
            | "inspect-wait"
            | "jsx-factory"
            | "jsx-fragment"
            | "jsx-import-source"
            | "jsx-runtime"
            | "jsx-side-effects"
            | "loader"
            | "main-fields"
            | "max-http-header-size"
            | "no-addons"
            | "no-clear-screen"
            | "no-deprecation"
            | "no-env-file"
            | "no-exit-on-error"
            | "no-install"
            | "no-macros"
            | "parallel"
            | "port"
            | "preload"
            | "prefer-latest"
            | "prefer-offline"
            | "preserve-symlinks"
            | "preserve-symlinks-main"
            | "print"
            | "redis-preconnect"
            | "require"
            | "shell"
            | "smol"
            | "sql-preconnect"
            | "throw-deprecation"
            | "title"
            | "tsconfig-override"
            | "unhandled-rejections"
            | "use-bundled-ca"
            | "use-openssl-ca"
            | "use-system-ca"
            | "user-agent"
            | "watch"
            | "workspaces"
            | "zero-fill-buffers"
    ) || matches!(arg, "-b" | "-e" | "-i" | "-p" | "-r")
}

fn looks_like_bun_file_entrypoint(arg: &str) -> bool {
    arg == "-"
        || arg.starts_with("./")
        || arg.starts_with("../")
        || arg.starts_with('/')
        || matches!(
            Path::new(arg).extension().and_then(OsStr::to_str),
            Some("cjs" | "cts" | "js" | "jsx" | "mjs" | "mts" | "tsx" | "ts")
        )
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
        "owner" | "pkg" | "publish" | "search" | "set-script" | "token" | "unpublish" | "whoami"
    )
}

fn global_flag_takes_value(name: &str) -> bool {
    matches!(
        name,
        "cache" | "color" | "loglevel" | "prefix" | "registry" | "userconfig"
    )
}

fn global_package_flag_takes_value(name: &str) -> bool {
    global_flag_takes_value(name) || install_flag_takes_value(name)
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

fn short_global_package_flag_takes_value(arg: &str) -> bool {
    short_global_flag_takes_value(arg) || short_install_flag_takes_value(arg)
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
