use anyhow::Result;
use aubeshim::{Cli, Invocation};
use clap::{CommandFactory, Parser};
use std::ffi::OsString;
use std::io::{self, Write};

fn main() {
    if let Err(err) = run() {
        // The reader went away, such as a login shell that exited before
        // reading `eval "$(aubeshim activate ...)"`. Exit quietly, as a C tool
        // would on SIGPIPE, rather than panicking on the failed write.
        if is_broken_pipe(&err) {
            std::process::exit(0);
        }
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain()
        .filter_map(|cause| cause.downcast_ref::<io::Error>())
        .any(|io_err| io_err.kind() == io::ErrorKind::BrokenPipe)
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
        Some(aubeshim::Command::Activate {
            shell,
            persistent,
            shim_dir,
        }) => {
            let dir = shim_dir.unwrap_or_else(aubeshim::default_shim_dir);
            write!(
                io::stdout(),
                "{}",
                aubeshim::shell_init(shell, &dir, persistent)
            )?;
            Ok(())
        }
        Some(aubeshim::Command::Install { force, shim_dir }) => {
            let dir = shim_dir.unwrap_or_else(aubeshim::default_shim_dir);
            let installed = aubeshim::install_shims(&dir, force)?;
            let mut out = io::stdout().lock();
            for path in installed {
                writeln!(out, "installed {}", path.display())?;
            }
            writeln!(out)?;
            writeln!(
                out,
                "Add aubeshim activation after mise or any other tool manager in your shell startup:"
            )?;
            writeln!(out)?;
            writeln!(out, "  zsh:  eval \"$(aubeshim activate zsh)\"")?;
            writeln!(out, "  bash: eval \"$(aubeshim activate bash)\"")?;
            writeln!(out, "  fish: aubeshim activate fish | source")?;
            writeln!(out, "  sh:   eval \"$(aubeshim activate sh)\"")?;
            writeln!(out)?;
            writeln!(
                out,
                "If mise runs without --shims, use --persistent so aubeshim stays ahead of mise hook-env:"
            )?;
            writeln!(
                out,
                "  zsh:  eval \"$(aubeshim activate zsh --persistent)\""
            )?;
            Ok(())
        }
        Some(aubeshim::Command::Uninstall { shim_dir }) => {
            let dir = shim_dir.unwrap_or_else(aubeshim::default_shim_dir);
            let mut out = io::stdout().lock();
            for path in aubeshim::uninstall_shims(&dir)? {
                writeln!(out, "removed {}", path.display())?;
            }
            Ok(())
        }
        None => {
            Cli::command().print_help()?;
            writeln!(io::stdout())?;
            Ok(())
        }
    }
}
