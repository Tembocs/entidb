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
//! - `backup` - Create or restore database backups
//! - `migrate` - Run database migrations

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

    /// Create or restore database backups
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },

    /// Run database migrations
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
}

#[derive(Subcommand)]
enum BackupAction {
    /// Create a backup of the database
    Create {
        /// Output file path
        #[arg(short, long)]
        output: PathBuf,

        /// Include tombstones in backup
        #[arg(short, long)]
        include_tombstones: bool,
    },

    /// Restore database from a backup
    Restore {
        /// Input backup file path
        #[arg(short, long)]
        input: PathBuf,

        /// Overwrite existing database
        #[arg(short, long)]
        force: bool,
    },

    /// Validate a backup file
    Validate {
        /// Backup file to validate
        #[arg(short, long)]
        input: PathBuf,
    },

    /// Show backup metadata
    Info {
        /// Backup file to inspect
        #[arg(short, long)]
        input: PathBuf,
    },
}

#[derive(Subcommand)]
enum MigrateAction {
    /// Show current migration status
    Status,

    /// List all registered migrations
    List,

    /// Run pending migrations
    Run {
        /// Run only up to this version
        #[arg(short, long)]
        to_version: Option<u64>,

        /// Dry run - show what would be done
        #[arg(short, long)]
        dry_run: bool,
    },
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
        Commands::Backup { action } => {
            let path = cli.path.ok_or("Database path required for backup")?;
            match action {
                BackupAction::Create {
                    output,
                    include_tombstones,
                } => {
                    commands::backup::create(&path, &output, include_tombstones)?;
                }
                BackupAction::Restore { input, force } => {
                    commands::backup::restore(&path, &input, force)?;
                }
                BackupAction::Validate { input } => {
                    commands::backup::validate(&input)?;
                }
                BackupAction::Info { input } => {
                    commands::backup::info(&input)?;
                }
            }
        }
        Commands::Migrate { action } => {
            let path = cli.path.ok_or("Database path required for migrate")?;
            match action {
                MigrateAction::Status => {
                    commands::migrate::status(&path)?;
                }
                MigrateAction::List => {
                    commands::migrate::list(&path)?;
                }
                MigrateAction::Run {
                    to_version,
                    dry_run,
                } => {
                    commands::migrate::run(&path, to_version, dry_run)?;
                }
            }
        }
        Commands::Version => {
            println!("EntiDB CLI v{}", env!("CARGO_PKG_VERSION"));
            println!("EntiDB Core v{}", entidb_core::VERSION);
        }
    }

    Ok(())
}
