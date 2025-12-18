# EntiDB CLI Guide

The EntiDB CLI provides command-line tools for database management, diagnostics, and maintenance.

## Installation

The CLI is built as part of the EntiDB workspace:

```bash
cargo install --path crates/entidb_cli
```

Or run directly:

```bash
cargo run -p entidb_cli -- <command>
```

---

## Commands

### `entidb inspect`

Display database statistics and metadata.

```bash
entidb -p /path/to/db inspect
```

**Options:**

| Option | Description |
|--------|-------------|
| `-c, --collections` | Show detailed collection information |
| `-s, --segments` | Show segment details |
| `-f, --format <FORMAT>` | Output format: `text` (default) or `json` |

**Example Output:**

```
EntiDB Database Inspection
==========================

Path: /data/mydb

Storage:
  WAL size:      12.3 KB
  Segment size:  1.2 MB
  Total size:    1.2 MB

Records:
  WAL records:     156
  Segment records: 10432

Entities:
  Live entities: 10000
  Tombstones:    432
```

**JSON Output:**

```bash
entidb -p /path/to/db inspect --format json
```

```json
{
  "path": "/data/mydb",
  "wal_size": 12583,
  "segment_size": 1258291,
  "total_size": 1270874,
  "wal_record_count": 156,
  "segment_record_count": 10432,
  "entity_count": 10000,
  "tombstone_count": 432
}
```

---

### `entidb verify`

Verify database integrity by checking checksums and structure.

```bash
entidb -p /path/to/db verify
```

**Options:**

| Option | Description |
|--------|-------------|
| `-w, --wal` | Check WAL records only |
| `-s, --segments` | Check segment records only |
| `-a, --all` | Check all (default if no flags) |

**Example Output:**

```
Verifying database at "/data/mydb"

Checking WAL...
  WAL records checked: 156, valid: 156, corrupt: 0
Checking segments...
  Segments records checked: 10432, valid: 10432, corrupt: 0

✓ Database verification passed
```

**Corruption Detection:**

```
Checking WAL...
  WAL records checked: 156, valid: 154, corrupt: 2
    ERROR: CRC mismatch at offset 12483: stored=a1b2c3d4, computed=d4c3b2a1
    ERROR: Truncated record at offset 14523

✗ Database verification failed
```

---

### `entidb compact`

Compact segments to reclaim space by removing obsolete versions and tombstones.

```bash
entidb -p /path/to/db compact
```

**Options:**

| Option | Description |
|--------|-------------|
| `-r, --remove-tombstones` | Remove all tombstones |
| `-d, --dry-run` | Show what would be done without making changes |

**Dry Run:**

```bash
entidb -p /path/to/db compact --dry-run
```

```
Compacting segments at "/data/mydb"
(dry run - no changes will be made)

Compaction Analysis:
  Input records:     15000
  Output records:    10000
  Tombstones:        500 (will be kept)
  Obsolete versions: 4500

  Size before: 1.8 MB
  Size after:  1.2 MB
  Space saved: 614 KB (34.1%)
```

**With Tombstone Removal:**

```bash
entidb -p /path/to/db compact --remove-tombstones
```

---

### `entidb dump-oplog`

Dump WAL records for debugging and analysis.

```bash
entidb -p /path/to/db dump-oplog
```

**Options:**

| Option | Description |
|--------|-------------|
| `-l, --limit <N>` | Maximum number of records to dump |
| `-o, --offset <N>` | Start from this byte offset |
| `-f, --format <FORMAT>` | Output format: `text` (default) or `json` |

**Example Output:**

```
WAL Records (50 total)
================

[00000000] BEGIN      txid=1
[00000019] PUT        txid=1 collection=1 entity=a1b2c3d4... payload=42 bytes
[00000089] COMMIT     txid=1 seq=1
[00000105] BEGIN      txid=2
[00000124] DELETE     txid=2 collection=1 entity=e5f6g7h8...
[00000172] COMMIT     txid=2 seq=2
```

**JSON Output:**

```bash
entidb -p /path/to/db dump-oplog --format json --limit 2
```

```json
[
  {
    "offset": 0,
    "record_type": "BEGIN",
    "txid": 1
  },
  {
    "offset": 25,
    "record_type": "PUT",
    "txid": 1,
    "collection_id": 1,
    "entity_id": "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6",
    "payload_size": 42
  }
]
```

---

### `entidb version`

Show version information.

```bash
entidb version
```

```
EntiDB CLI v0.1.0
EntiDB Core v0.1.0
```

---

### `entidb backup`

Create, restore, or validate database backups.

#### `entidb backup create`

Create a backup of the database.

```bash
entidb -p /path/to/db backup create --output /path/to/backup.endb
```

**Options:**

| Option | Description |
|--------|-------------|
| `-o, --output <PATH>` | Output backup file path (required) |
| `-i, --include-tombstones` | Include tombstones in the backup |

**Example:**

```bash
entidb -p /data/mydb backup create -o /backups/mydb-20241218.endb
```

```
✓ Backup created successfully
  Path: /backups/mydb-20241218.endb
  Size: 1258291 bytes
  Records: 10432
  Timestamp: 2024-12-18T10:30:00Z
```

#### `entidb backup restore`

Restore a database from a backup file.

```bash
entidb -p /path/to/newdb backup restore --input /path/to/backup.endb
```

**Options:**

| Option | Description |
|--------|-------------|
| `-i, --input <PATH>` | Input backup file path (required) |
| `-f, --force` | Overwrite existing database |

**Example:**

```bash
entidb -p /data/restored backup restore -i /backups/mydb-20241218.endb
```

```
✓ Database restored successfully
  Path: /data/restored
  Records restored: 10432
  From backup created: 2024-12-18T10:30:00Z
```

#### `entidb backup validate`

Validate a backup file without restoring.

```bash
entidb backup validate --input /path/to/backup.endb
```

```
✓ Backup is valid
  File size: 1258291 bytes
  Record count: 10432
  Sequence: 5678
  Created: 2024-12-18T10:30:00Z
```

#### `entidb backup info`

Show backup metadata.

```bash
entidb backup info --input /path/to/backup.endb
```

---

### `entidb migrate`

Manage database schema migrations.

#### `entidb migrate status`

Show current migration status.

```bash
entidb -p /path/to/db migrate status
```

```
Migration Status
================
  Current version: 3
  Applied migrations: 3

Applied Migrations:
  v1: create_users_collection (applied at 2024-12-01)
  v2: add_email_index (applied at 2024-12-10)
  v3: add_settings_collection (applied at 2024-12-15)
```

#### `entidb migrate list`

List all registered migrations.

```bash
entidb -p /path/to/db migrate list
```

```
Registered Migrations
====================
  v1: create_users_collection [✓ applied]
  v2: add_email_index [✓ applied]
  v3: add_settings_collection [✓ applied]
  v4: add_audit_log [○ pending]
```

#### `entidb migrate run`

Run pending migrations.

```bash
entidb -p /path/to/db migrate run
```

**Options:**

| Option | Description |
|--------|-------------|
| `-t, --to-version <N>` | Run only up to this version |
| `-d, --dry-run` | Show what would be done |

**Dry Run:**

```bash
entidb -p /path/to/db migrate run --dry-run
```

```
Dry run - would apply 1 migration(s):
  v4: add_audit_log
```

**Run:**

```bash
entidb -p /path/to/db migrate run
```

```
Running 1 migration(s)...

✓ Successfully applied 1 migration(s)
  Final version: 4
```

---

## Global Options

These options apply to all commands:

| Option | Description |
|--------|-------------|
| `-p, --path <PATH>` | Path to the database directory |
| `-v, --verbose` | Enable verbose/debug output |
| `-h, --help` | Print help information |
| `-V, --version` | Print version |

---

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Error (see error message) |

---

## Examples

### Backup Workflow

```bash
# 1. Verify database is healthy
entidb -p /data/mydb verify

# 2. Compact before backup
entidb -p /data/mydb compact --remove-tombstones

# 3. Copy files for backup
cp -r /data/mydb /backup/mydb-$(date +%Y%m%d)
```

### Debugging

```bash
# Check database health
entidb -p /data/mydb verify -v

# Inspect recent operations
entidb -p /data/mydb dump-oplog --limit 100

# Get detailed stats
entidb -p /data/mydb inspect --collections --format json
```

### Space Reclamation

```bash
# Check how much space can be saved
entidb -p /data/mydb compact --dry-run

# Perform compaction
entidb -p /data/mydb compact --remove-tombstones
```

---

## Troubleshooting

### "Database path required"

Ensure you specify the `-p` option:

```bash
entidb -p /path/to/db inspect
```

### "No database found"

Verify the path contains EntiDB files (`wal.log`, `segments.dat`):

```bash
ls -la /path/to/db
```

### "Verification failed"

If verification fails, the database may be corrupted. Options:
1. Restore from backup
2. Contact support with the verification output

---

## See Also

- [Quick Start Guide](quickstart.md)
- [API Reference](api_reference.md)
- [File Format Specification](file_format.md)
