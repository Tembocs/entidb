# entidb_cli

CLI tools for EntiDB.

## Overview

This crate provides command-line utilities for inspecting, verifying, and maintaining
EntiDB databases.

## Installation

```bash
cargo install entidb_cli
```

## Commands

### inspect

Inspect a database file or directory:

```bash
entidb inspect ./my_database
```

### verify

Verify database integrity (checksums, WAL, segments):

```bash
entidb verify ./my_database
```

### compact

Trigger database compaction:

```bash
entidb compact ./my_database
```

### dump-oplog

Dump the logical oplog for debugging:

```bash
entidb dump-oplog ./my_database
```

### stats

Display database statistics:

```bash
entidb stats ./my_database
```

## Output Formats

Most commands support `--format` for output format:

- `text` (default) - Human-readable output
- `json` - Machine-readable JSON

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
