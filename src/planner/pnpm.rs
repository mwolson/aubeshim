use super::{
    command_index, dlx, has_global_marker, plan_global_package_action, plan_mise_global_outdated,
    GlobalPackageAction, Plan, Target,
};
use crate::config::GlobalPackages;
use std::ffi::OsString;

pub(super) fn plan(args: &[OsString], global_packages: GlobalPackages) -> Plan {
    if let Some(command_idx) = command_index(args) {
        let command = args[command_idx].to_string_lossy().to_ascii_lowercase();
        if command == "dlx" {
            return dlx::plan_pnpm_dlx_with_prefix(
                &args[..command_idx],
                &args[command_idx + 1..],
                Target::RealPnpm,
                args,
            );
        }
        if command == "outdated" && has_global_marker(args) {
            return plan_mise_global_outdated(&args[command_idx + 1..]);
        }
        if has_global_marker(args) {
            if let Some(action) = pnpm_global_package_action(&command) {
                return plan_global_package_action(
                    global_packages,
                    action,
                    &args[command_idx + 1..],
                )
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

fn pnpm_global_package_action(command: &str) -> Option<GlobalPackageAction> {
    match command {
        "add" | "i" | "install" => Some(GlobalPackageAction::Use),
        "remove" | "rm" | "uninstall" => Some(GlobalPackageAction::Unuse),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::test_support::{
        mise_global_outdated_args, mise_global_unuse_args, mise_global_use_args, os, strings,
    };

    #[test]
    fn pnpm_passes_through_to_aube() {
        let plan = plan(&os(&["install", "--frozen-lockfile"]), GlobalPackages::Mise);

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["install", "--frozen-lockfile"]);
    }

    #[test]
    fn pnpm_dlx_uses_aube_dlx_with_supported_flags() {
        let plan = plan(
            &os(&["dlx", "-s", "vite", "--version"]),
            GlobalPackages::Mise,
        );

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(
            strings(&plan.args),
            vec!["dlx", "--silent", "vite", "--version"]
        );
    }

    #[test]
    fn pnpm_dlx_allow_build_uses_aube_dlx() {
        for (args, expected) in [
            (
                &["dlx", "--allow-build=esbuild", "vite"][..],
                vec!["dlx", "--allow-build=esbuild", "vite"],
            ),
            (
                &["dlx", "--allow-build", "esbuild", "vite"][..],
                vec!["dlx", "--allow-build=esbuild", "vite"],
            ),
        ] {
            let plan = plan(&os(args), GlobalPackages::Mise);

            assert_eq!(plan.target, Target::Aube);
            assert_eq!(strings(&plan.args), expected);
        }
    }

    #[test]
    fn pnpm_global_package_operations_use_mise() {
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
        ] {
            let plan = plan(&os(args), GlobalPackages::Mise);

            assert_eq!(plan.target, Target::Mise);
            assert_eq!(strings(&plan.args), expected);
        }
    }

    #[test]
    fn pnpm_global_outdated_uses_mise() {
        let plan = plan(&os(&["outdated", "-g"]), GlobalPackages::Mise);

        assert_eq!(plan.target, Target::Mise);
        assert_eq!(strings(&plan.args), mise_global_outdated_args(&[]));
    }
}
