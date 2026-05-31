use super::{
    command_index, has_global_marker, plan_mise_global_outdated, plan_mise_global_package_action,
    GlobalPackageAction, Plan, Target,
};
use std::ffi::OsString;

pub(super) fn plan(args: &[OsString]) -> Plan {
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
        out.extend(translate_install_args(rest));
    } else {
        out.extend_from_slice(rest);
    }
    Plan {
        target: Target::Aube,
        args: out,
    }
}

fn translate_install_args(args: &[OsString]) -> Vec<OsString> {
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
