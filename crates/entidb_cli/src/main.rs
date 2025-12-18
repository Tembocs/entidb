//! EntiDB CLI
//!
//! Command-line tools for EntiDB database management.
//!
//! # Commands
//!
//! - `inspect` - Display database statistics and metadata
//! - `verify` - Verify database integrity
//! - `compact` - Compact segments to reclaim space
//! - `dump-oplog` - Dump WAL records for debugging

mod commands;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

/// EntiDB command-line database tools.
#[derive(Parser)]
#[command(name = "entidb")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to the database directory
    #[arg(global = true, short, long)]
    path: Option<PathBuf>,

    /// Enable verbose output
    #[arg(global = true, short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Display database statistics and metadata
    Inspect {
        /// Show detailed collection information
        #[arg(short, long)]
        collections: bool,

        /// Show segment details
        #[arg(short, long)]
        segments: bool,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        format: String,
    },

    /// Verify database integrity
    Verify {
        /// Check WAL records
        #[arg(short, long)]
        wal: bool,

        /// Check segment records
        #[arg(short, long)]
        segments: bool,

        /// Check all (default if no flags specified)
        #[arg(short, long)]
        all: bool,
    },

    /// Compact segments to reclaim space
    Compact {
        /// Remove all tombstones
        #[arg(short, long)]
        remove_tombstones: bool,

        /// Dry run - show what would be done
        #[arg(short, long)]
        dry_run: bool,
    },

    /// Dump WAL records for debugging
    DumpOplog {
        /// Maximum number of records to dump
        #[arg(short, long)]
        limit: Option<usize>,

        /// Start from this offset
        #[arg(short, long, default_value = "0")]
        offset: u64,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        format: String,
    },

    /// Show version information
    Version,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };
    tracing_subscriber::fmt().with_env_filter(filter).init();

    match cli.command {
        Commands::Inspect {
            collections,
            segments,
            format,
        } => {
            let path = cli.path.ok_or("Database path required for inspect")?;
            commands::inspect::run(&path, collections, segments, &format)?;
        }
        Commands::Verify { wal, segments, all } => {
            let path = cli.path.ok_or("Database path required for verify")?;
            let check_all = all || (!wal && !segments);
            commands::verify::run(&path, wal || check_all, segments || check_all)?;
        }
        Commands::Compact {
            remove_tombstones,
            dry_run,
        } => {
            let path = cli.path.ok_or("Database path required for compact")?;
            commands::compact::run(&path, remove_tombstones, dry_run)?;
        }
        Commands::DumpOplog {
            limit,
            offset,
            format,
        } => {
            let path = cli.path.ok_or("Database path required for dump-oplog")?;
            commands::dump_oplog::run(&path, limit, offset, &format)?;
        }
        Commands::Version => {
            println!("EntiDB CLI v{}", env!("CARGO_PKG_VERSION"));
            println!("EntiDB Core v{}", entidb_core::VERSION);
        }
    }

    Ok(())
}
