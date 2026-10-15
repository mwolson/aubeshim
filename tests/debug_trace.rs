#![cfg(unix)]

mod common;

use common::Sandbox;
use std::fs;

#[test]
fn debug_trace_shows_invocation_and_route() {
    let sandbox = Sandbox::new();
    let staging = sandbox.path("staging dir");
    fs::create_dir_all(&staging).unwrap();
    let staging = staging.to_string_lossy().into_owned();
    let home = sandbox.path("home");

    let output = sandbox.npm_output(
        &home,
        &["install", "--prefix", &staging, "is-number@7.0.0"],
        &[("AUBESHIM_DEBUG", "1")],
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    let real_npm = sandbox.path("bin/real-npm");

    assert_eq!(
        stderr.lines().collect::<Vec<_>>(),
        vec![
            format!(
                "aubeshim: npm install --prefix '{staging}' is-number@7.0.0 (cwd={}, shim=true)",
                home.display()
            ),
            format!(
                "aubeshim: -> real npm: {} install --prefix '{staging}' is-number@7.0.0",
                real_npm.display()
            ),
        ]
    );
}

#[test]
fn debug_trace_lists_injected_aube_env() {
    let sandbox = Sandbox::new();
    let home = sandbox.path("home");

    let output = sandbox.npm_output(
        &home,
        &["run", "build"],
        &[
            ("AUBESHIM_DEBUG", "1"),
            ("AUBE_PACKAGE_IMPORT_METHOD", "copy"),
        ],
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    let aube = sandbox.path("bin/aube");

    assert_eq!(
        stderr.lines().last().unwrap(),
        format!(
            "aubeshim: -> aube: AUBE_NODE_LINKER=hoisted AUBE_MINIMUM_PACKAGE_AGE=0 {} run build",
            aube.display()
        )
    );
}

#[test]
fn debug_trace_is_off_by_default_and_when_empty() {
    let sandbox = Sandbox::new();
    let home = sandbox.path("home");

    for envs in [&[][..], &[("AUBESHIM_DEBUG", "")][..]] {
        let output = sandbox.npm_output(&home, &["run", "build"], envs);
        assert!(output.stderr.is_empty(), "envs={envs:?}");
    }
}
