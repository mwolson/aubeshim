use super::{
    command_index, has_global_marker, install_flag_takes_value, long_flag_name,
    plan_mise_global_outdated, plan_mise_global_package_action, push_omit_translation,
    short_install_flag_takes_value, GlobalPackageAction, Plan, Target,
};
use std::ffi::OsString;

pub(super) fn plan(args: &[OsString]) -> Plan {
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
        let Some(translated_prefix) = translate_project_install_args(prefix) else {
            return Plan {
                target: Target::RealNpm,
                args: args.to_vec(),
            };
        };
        let Some(translated_rest) = translate_install_package_args(rest) else {
            return Plan {
                target: Target::RealNpm,
                args: args.to_vec(),
            };
        };
        let mut out = Vec::with_capacity(args.len());
        out.extend(translated_prefix);
        out.push(OsString::from("add"));
        out.extend(translated_rest);
        return Plan {
            target: Target::Aube,
            args: out,
        };
    }

    let mut out = Vec::with_capacity(args.len());
    let translated_prefix = if matches!(command.as_str(), "ci" | "install" | "i" | "in") {
        match translate_project_install_args(prefix) {
            Some(translated) => translated,
            None => {
                return Plan {
                    target: Target::RealNpm,
                    args: args.to_vec(),
                };
            }
        }
    } else {
        prefix.to_vec()
    };
    out.extend(translated_prefix);
    out.push(OsString::from(normalize_command(&command)));
    if matches!(command.as_str(), "ci" | "install" | "i" | "in") {
        let Some(translated) = translate_project_install_args(rest) else {
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

fn install_has_packages(args: &[OsString]) -> bool {
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "--" {
            return i + 1 < args.len();
        }
        if arg == "--progress" {
            if args
                .get(i + 1)
                .is_some_and(|value| is_bool_value(&value.to_string_lossy()))
            {
                i += 2;
            } else {
                i += 1;
            }
            continue;
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

fn has_json_marker(args: &[OsString]) -> bool {
    args.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        arg == "--json" || arg.starts_with("--json=")
    })
}

fn translate_install_package_args(args: &[OsString]) -> Option<Vec<OsString>> {
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--" {
            out.extend_from_slice(&args[i..]);
            break;
        }
        if skip_install_layout_arg(args, &mut i)? {
            continue;
        }
        if skip_install_noop_arg(args, &mut i)? {
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

fn translate_project_install_args(args: &[OsString]) -> Option<Vec<OsString>> {
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--" {
            out.extend_from_slice(&args[i..]);
            break;
        }
        if skip_install_layout_arg(args, &mut i)? {
            continue;
        }
        if skip_install_noop_arg(args, &mut i)? {
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

fn skip_install_layout_arg(args: &[OsString], i: &mut usize) -> Option<bool> {
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

fn skip_install_noop_arg(args: &[OsString], i: &mut usize) -> Option<bool> {
    let arg = args[*i].to_string_lossy();
    if arg == "--cache" {
        args.get(*i + 1)?;
        *i += 2;
        return Some(true);
    }
    if arg.starts_with("--cache=") {
        *i += 1;
        return Some(true);
    }

    if arg == "--progress" {
        *i += 1;
        if args
            .get(*i)
            .is_some_and(|value| is_bool_value(&value.to_string_lossy()))
        {
            *i += 1;
        }
        return Some(true);
    }
    if arg == "--no-progress" {
        *i += 1;
        return Some(true);
    }
    if let Some(value) = arg.strip_prefix("--progress=") {
        if is_bool_value(value) {
            *i += 1;
            return Some(true);
        }
        return None;
    }

    if arg == "--no-audit" || arg == "--no-fund" {
        *i += 1;
        return Some(true);
    }
    for name in ["audit", "fund"] {
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

fn is_bool_value(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "true" | "false")
}

fn normalize_command(command: &str) -> &'static str {
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
