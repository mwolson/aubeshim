use crate::shell::Shell;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "aubeshim",
    version,
    about = "Install and run aube-backed package-manager shims"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print shell code that prepends the aubeshim shim directory to PATH
    Activate {
        /// Shell syntax to emit
        shell: Shell,
        /// Shim directory to put on PATH
        #[arg(long, value_name = "DIR")]
        shim_dir: Option<PathBuf>,
    },
    /// Create package-manager shims that point at this executable
    Install {
        /// Replace existing shim files
        #[arg(long)]
        force: bool,
        /// Directory where package-manager shims should be installed
        #[arg(long, value_name = "DIR")]
        shim_dir: Option<PathBuf>,
    },
    /// Remove package-manager shims
    Uninstall {
        /// Directory where package-manager shims were installed
        #[arg(long, value_name = "DIR")]
        shim_dir: Option<PathBuf>,
    },
}
