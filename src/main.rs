use anyhow::Result;
use aubeshim::{Cli, Invocation};
use clap::{CommandFactory, Parser};
use std::ffi::OsString;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<OsString> = std::env::args_os().collect();
    match aubeshim::invocation_from_argv0(args.first()) {
        Invocation::Shim(tool) => aubeshim::exec_shim(tool, &args[1..]),
        Invocation::Manager => dispatch(Cli::parse_from(args)),
    }
}

fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Some(aubeshim::Command::Activate { shell, shim_dir }) => {
            let dir = shim_dir.unwrap_or_else(aubeshim::default_shim_dir);
            print!("{}", aubeshim::shell_init(shell, &dir));
            Ok(())
        }
        Some(aubeshim::Command::Install { force, shim_dir }) => {
            let dir = shim_dir.unwrap_or_else(aubeshim::default_shim_dir);
            let installed = aubeshim::install_shims(&dir, force)?;
            for path in installed {
                println!("installed {}", path.display());
            }
            println!();
            println!(
                "Add aubeshim activation after mise or any other tool manager in your shell startup:"
            );
            println!();
            println!("  zsh:  eval \"$(aubeshim activate zsh)\"");
            println!("  bash: eval \"$(aubeshim activate bash)\"");
            println!("  fish: aubeshim activate fish | source");
            println!("  sh:   eval \"$(aubeshim activate sh)\"");
            Ok(())
        }
        Some(aubeshim::Command::Uninstall { shim_dir }) => {
            let dir = shim_dir.unwrap_or_else(aubeshim::default_shim_dir);
            for path in aubeshim::uninstall_shims(&dir)? {
                println!("removed {}", path.display());
            }
            Ok(())
        }
        None => {
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    }
}
