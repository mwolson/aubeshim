#![cfg(unix)]

mod common;

use common::Sandbox;
use std::fs;

#[test]
fn npm_install_into_bare_prefix_runs_real_npm() {
    let sandbox = Sandbox::new();
    let project = sandbox.path("project");
    let staging = project.join("tools/expo-device-hub/0.15.3");
    fs::create_dir_all(&staging).unwrap();
    fs::write(project.join("package.json"), "{}").unwrap();
    let staging = staging.to_string_lossy().into_owned();

    let stdout = sandbox.npm(
        &sandbox.path("home"),
        &[
            "install",
            "--prefix",
            &staging,
            "--no-fund",
            "--no-audit",
            "expo-device-hub@0.15.3",
        ],
    );

    assert_eq!(
        stdout,
        format!("real-npm install --prefix {staging} --no-fund --no-audit expo-device-hub@0.15.3")
    );
}

#[test]
fn npm_install_into_package_prefix_runs_aube() {
    let sandbox = Sandbox::new();
    let project = sandbox.path("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("package.json"), "{}").unwrap();
    let project = project.to_string_lossy().into_owned();

    let stdout = sandbox.npm(
        &sandbox.path("home"),
        &["install", "--prefix", &project, "--no-fund", "react"],
    );

    assert_eq!(stdout, format!("aube add --prefix {project} react"));
}
