mod cli;
mod config;
mod planner;
mod runtime;
mod shell;
mod shims;

pub use cli::{Cli, Command};
pub use config::{default_config_path, load_config, Config};
pub use planner::{plan_for, plan_for_config, Plan, Target};
pub use runtime::exec_shim;
pub use shell::{shell_init, Shell};
pub use shims::{
    default_shim_dir, install_shims, invocation_from_argv0, uninstall_shims, Invocation, ShimTool,
};

use std::env;
use std::path::PathBuf;

pub(crate) fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_config;
    use crate::runtime::{
        compare_dotted_versions, mise_version_from_output, missing_tool_error,
        unsupported_mise_error, version_from_output,
    };
    use std::{cmp::Ordering, ffi::OsString, fs, path::Path};

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

    fn mise_global_use_args(packages: &[&str]) -> Vec<String> {
        mise_global_package_args("use", packages)
    }

    fn mise_global_unuse_args(packages: &[&str]) -> Vec<String> {
        mise_global_package_args("unuse", packages)
    }

    fn mise_global_package_args(command: &str, packages: &[&str]) -> Vec<String> {
        let mut args = vec![command.to_owned(), "-g".to_owned()];
        args.extend(packages.iter().map(|arg| format!("npm:{arg}")));
        args
    }

    #[test]
    fn config_defaults_to_shimming() {
        let repo = repo_fixture();
        let plan = plan_for_config(
            ShimTool::Npm,
            &os(&["install"]),
            &Config::default(),
            &repo.cwd,
        )
        .unwrap();

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_shimmed_version_flags_are_aube_targets() {
        let repo = repo_fixture();
        let plan = plan_for_config(
            ShimTool::Npm,
            &os(&["--version"]),
            &Config::default(),
            &repo.cwd,
        )
        .unwrap();

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["--version"]);
    }

    #[test]
    fn bun_version_flag_is_normally_real_bun() {
        let plan = plan_for(ShimTool::Bun, &os(&["--version"]));

        assert_eq!(plan.target, Target::RealBun);
        assert_eq!(strings(&plan.args), vec!["--version"]);
    }

    #[test]
    fn config_ignored_version_flags_pass_through_to_real_tool() {
        let repo = repo_fixture();
        let config = Config {
            default: false,
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Npm, &os(&["--version"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["--version"]);
    }

    #[test]
    fn config_global_disable_passes_through_to_real_tool() {
        let repo = repo_fixture();
        let config = Config {
            enabled: false,
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Npm, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_global_disable_overrides_shim_glob() {
        let repo = repo_fixture();
        let config = Config {
            enabled: false,
            shim: vec![repo.root.to_string_lossy().into_owned()],
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Pnpm, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealPnpm);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_shim_glob_overrides_default_disable() {
        let repo = repo_fixture();
        let config = Config {
            default: false,
            shim: vec![repo.root.to_string_lossy().into_owned()],
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Pnpm, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_default_disable_passes_through_to_real_tool() {
        let repo = repo_fixture();
        let config = Config {
            default: false,
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Npm, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_ignore_glob_overrides_shim_glob() {
        let repo = repo_fixture();
        let pattern = repo.root.to_string_lossy().into_owned();
        let config = Config {
            enabled: true,
            default: true,
            ignore: vec![pattern.clone()],
            shim: vec![pattern],
        };
        let plan = plan_for_config(ShimTool::Yarn, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealYarn);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_globs_match_selected_ancestor_dirs() {
        let repo = repo_fixture();
        let config = Config {
            enabled: true,
            ignore: vec![format!("{}/*", repo.root.parent().unwrap().display())],
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Bun, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealBun);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_single_star_glob_matches_descendants_of_selected_dirs() {
        let repo = nested_repo_fixture();
        let config = Config {
            ignore: vec![format!(
                "{}/*",
                repo.root
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .display()
            )],
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Npm, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_double_star_glob_matches_nested_dirs() {
        let repo = nested_repo_fixture();
        let config = Config {
            ignore: vec![format!(
                "{}/**",
                repo.root
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .display()
            )],
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Npm, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn parses_config_file() {
        let config = parse_config(
            r#"
enabled = false
default = true
ignore = ["~/devel/work/broken-expo"]
shim = ["~/devel/work/*"]
"#,
            Path::new("/tmp/aubeshim-config.toml"),
        )
        .unwrap();

        assert!(!config.enabled);
        assert!(config.default);
        assert_eq!(config.ignore, vec!["~/devel/work/broken-expo"]);
        assert_eq!(config.shim, vec!["~/devel/work/*"]);
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
    fn npm_install_no_fund_with_packages_is_removed() {
        let plan = plan_for(ShimTool::Npm, &os(&["install", "react", "--no-fund"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["add", "react"]);
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
    fn npm_global_install_with_package_uses_mise() {
        let plan = plan_for(ShimTool::Npm, &os(&["-g", "install", "cowsay"]));

        assert_eq!(plan.target, Target::Mise);
        assert_eq!(strings(&plan.args), mise_global_use_args(&["cowsay"]));
    }

    #[test]
    fn npm_global_remove_uses_mise() {
        let remove = plan_for(ShimTool::Npm, &os(&["remove", "--global", "cowsay"]));

        assert_eq!(remove.target, Target::Mise);
        assert_eq!(strings(&remove.args), mise_global_unuse_args(&["cowsay"]));
    }

    #[test]
    fn npm_global_install_without_package_uses_real_npm() {
        let plan = plan_for(ShimTool::Npm, &os(&["install", "-g"]));

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["install", "-g"]);
    }

    #[test]
    fn npm_global_install_skips_package_manager_flags_for_mise() {
        let plan = plan_for(
            ShimTool::Npm,
            &os(&[
                "install",
                "-g",
                "--registry",
                "https://registry.npmjs.org",
                "--json",
                "@biomejs/biome@latest",
            ]),
        );

        assert_eq!(plan.target, Target::Mise);
        assert_eq!(
            strings(&plan.args),
            mise_global_use_args(&["@biomejs/biome@latest"])
        );
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
    fn npm_install_omit_filters_use_aube_equivalents() {
        let plan = plan_for(
            ShimTool::Npm,
            &os(&["ci", "--omit", "optional", "--omit=dev"]),
        );

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["ci", "--no-optional", "--prod"]);
    }

    #[test]
    fn npm_install_unsupported_omit_filter_uses_real_npm() {
        let plan = plan_for(ShimTool::Npm, &os(&["ci", "--omit=peer"]));

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["ci", "--omit=peer"]);
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
    fn npm_publish_uses_real_npm() {
        for args in [
            &["publish", "--access", "public"][..],
            &["unpublish", "aubeshim@0.0.0"][..],
        ] {
            let plan = plan_for(ShimTool::Npm, &os(args));

            assert_eq!(plan.target, Target::RealNpm);
            assert_eq!(strings(&plan.args), args);
        }
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
            let plan = plan_for(ShimTool::Pnpm, &os(args));

            assert_eq!(plan.target, Target::Mise);
            assert_eq!(strings(&plan.args), expected);
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
    fn bun_install_omit_optional_uses_aube_no_optional() {
        let plan = plan_for(
            ShimTool::Bun,
            &os(&["install", "--production", "--omit", "optional"]),
        );

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(
            strings(&plan.args),
            vec!["install", "--production", "--no-optional"]
        );
    }

    #[test]
    fn bun_install_unsupported_omit_filter_uses_real_bun() {
        let plan = plan_for(ShimTool::Bun, &os(&["install", "--omit=peer"]));

        assert_eq!(plan.target, Target::RealBun);
        assert_eq!(strings(&plan.args), vec!["install", "--omit=peer"]);
    }

    #[test]
    fn bun_run_uses_aube_run() {
        let plan = plan_for(ShimTool::Bun, &os(&["run", "dev"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["run", "dev"]);
    }

    #[test]
    fn bun_run_with_runtime_flags_uses_real_bun() {
        for args in [
            &["--watch", "run", "dev"][..],
            &["run", "--watch", "dev"][..],
            &["run", "--bun", "dev"][..],
            &["run", "-b", "dev"][..],
            &["run", "--preload", "./setup.ts", "dev"][..],
        ] {
            let plan = plan_for(ShimTool::Bun, &os(args));

            assert_eq!(plan.target, Target::RealBun);
            assert_eq!(strings(&plan.args), args);
        }
    }

    #[test]
    fn bun_run_file_entrypoints_use_real_bun() {
        for args in [
            &["run", "./src/app.ts"][..],
            &["run", "../scripts/dev.tsx"][..],
            &["run", "/tmp/app.mjs"][..],
            &["run", "server.jsx"][..],
        ] {
            let plan = plan_for(ShimTool::Bun, &os(args));

            assert_eq!(plan.target, Target::RealBun);
            assert_eq!(strings(&plan.args), args);
        }
    }

    #[test]
    fn bun_run_script_args_still_use_aube() {
        for args in [
            &["run", "dev", "--watch"][..],
            &["run", "dev", "--", "--watch"][..],
        ] {
            let plan = plan_for(ShimTool::Bun, &os(args));

            assert_eq!(plan.target, Target::Aube);
            assert_eq!(strings(&plan.args), args);
        }
    }

    #[test]
    fn bun_global_package_operations_use_mise() {
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
            let plan = plan_for(ShimTool::Bun, &os(args));

            assert_eq!(plan.target, Target::Mise);
            assert_eq!(strings(&plan.args), expected);
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
        for args in [
            &["-e", "console.log(1)"][..],
            &["build", "./src/app.ts"][..],
            &["pm", "cache"][..],
            &["test", "src/app.test.ts"][..],
        ] {
            let plan = plan_for(ShimTool::Bun, &os(args));

            assert_eq!(plan.target, Target::RealBun);
            assert_eq!(strings(&plan.args), args);
        }
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
    fn yarn_install_ignore_optional_uses_aube_no_optional() {
        let plan = plan_for(ShimTool::Yarn, &os(&["install", "--ignore-optional"]));

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["install", "--no-optional"]);
    }

    #[test]
    fn yarn_run_style_script_passes_to_aube_external_script() {
        let plan = plan_for(ShimTool::Yarn, &os(&["dev", "--host"]));

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
            let plan = plan_for(ShimTool::Yarn, &os(args));

            assert_eq!(plan.target, Target::Mise);
            assert_eq!(strings(&plan.args), expected);
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
    fn parses_tool_version_output() {
        assert_eq!(version_from_output("aube 0.3.3\n"), Some("0.3.3"));
        assert_eq!(version_from_output("1.2.3\n"), Some("1.2.3"));
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

    struct RepoFixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        cwd: PathBuf,
    }

    fn repo_fixture() -> RepoFixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        let cwd = root.join("packages/app");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        RepoFixture {
            _dir: dir,
            root,
            cwd,
        }
    }

    fn nested_repo_fixture() -> RepoFixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repos/work/nested/app");
        let cwd = root.join("packages/app");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        RepoFixture {
            _dir: dir,
            root,
            cwd,
        }
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
