use super::{
    command_index, has_global_marker, plan_compat_fallback, plan_global_outdated,
    plan_global_package_action, GlobalPackageAction, Plan, Target,
};
use crate::globals::ResolvedGlobalBackend;
use crate::shims::ShimTool;
use std::ffi::OsString;

pub(super) fn plan(args: &[OsString], global_backend: ResolvedGlobalBackend) -> Plan {
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
        return plan_global_outdated(global_backend, rest);
    }

    if has_global_marker(args) {
        if let Some(action) = yarn_global_package_action(&command) {
            return plan_global_package_action(global_backend, action, rest).unwrap_or_else(|| {
                Plan {
                    target: Target::RealYarn,
                    args: args.to_vec(),
                }
            });
        }
    }

    if let Some(plan) = plan_compat_fallback(ShimTool::Yarn, args) {
        return plan;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::test_support::{
        mise_global_outdated_args, mise_global_unuse_args, mise_global_use_args, os, strings,
    };

    #[test]
    fn yarn_without_args_installs() {
        let plan = plan(&os(&[]), ResolvedGlobalBackend::Mise);

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn yarn_version_flag_passes_through() {
        let plan = plan(&os(&["--version"]), ResolvedGlobalBackend::Mise);

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["--version"]);
    }

    #[test]
    fn yarn_install_ignore_optional_uses_aube_no_optional() {
        let plan = plan(
            &os(&["install", "--ignore-optional"]),
            ResolvedGlobalBackend::Mise,
        );

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["install", "--no-optional"]);
    }

    #[test]
    fn yarn_run_style_script_passes_to_aube_external_script() {
        let plan = plan(&os(&["dev", "--host"]), ResolvedGlobalBackend::Mise);

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["dev", "--host"]);
    }

    #[test]
    fn yarn_global_package_operations_use_mise() {
        for (args, expected) in [
            (
                &["add", "-g", "cowsay"][..],
                mise_global_use_args(&["cowsay"]),
            ),
            (
                &["install", "--global", "typescript"][..],
                mise_global_use_args(&["typescript"]),
            ),
            (
                &["remove", "-g", "cowsay"][..],
                mise_global_unuse_args(&["cowsay"]),
            ),
            (
                &["up", "-g", "eslint"][..],
                mise_global_use_args(&["eslint"]),
            ),
        ] {
            let plan = plan(&os(args), ResolvedGlobalBackend::Mise);

            assert_eq!(plan.target, Target::Mise);
            assert_eq!(strings(&plan.args), expected);
        }
    }

    #[test]
    fn yarn_global_outdated_uses_mise() {
        let plan = plan(
            &os(&["outdated", "--global=true", "oxlint"]),
            ResolvedGlobalBackend::Mise,
        );

        assert_eq!(plan.target, Target::Mise);
        assert_eq!(
            strings(&plan.args),
            mise_global_outdated_args(&["npm:oxlint"])
        );
    }

    #[test]
    fn yarn_only_command_uses_real_yarn() {
        let plan = plan(&os(&["plugin", "list"]), ResolvedGlobalBackend::Mise);

        assert_eq!(plan.target, Target::RealYarn);
        assert_eq!(strings(&plan.args), vec!["plugin", "list"]);
    }

    #[test]
    fn yarn_whoami_uses_real_npm() {
        let plan = plan(&os(&["whoami"]), ResolvedGlobalBackend::Mise);

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["whoami"]);
    }

    #[test]
    fn yarn_login_and_logout_use_real_yarn() {
        for args in [&["login"][..], &["logout"][..]] {
            let plan = plan(&os(args), ResolvedGlobalBackend::Mise);

            assert_eq!(plan.target, Target::RealYarn, "args={args:?}");
            assert_eq!(strings(&plan.args), args);
        }
    }
}
