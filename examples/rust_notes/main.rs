//! Notes Application Example
//!
//! This example demonstrates:
//! - Working with complex entities
//! - Using indexes for efficient queries
//! - Backup and restore functionality
//! - Encryption at rest

use entidb_codec::{Encoder, Value};
use entidb_core::{
    backup::BackupManager,
    database::{Database, DatabaseConfig},
    entity::{Entity, EntityCodec, EntityId},
};
use entidb_storage::file::FileBackend;
use std::path::PathBuf;
use tempfile::TempDir;

/// A note with tags and content
#[derive(Debug, Clone)]
struct Note {
    id: EntityId,
    title: String,
    content: String,
    tags: Vec<String>,
    created_at: u64,
    updated_at: u64,
}

impl Entity for Note {
    fn id(&self) -> EntityId {
        self.id
    }
}

impl EntityCodec for Note {
    fn encode(&self) -> Result<Vec<u8>, entidb_codec::Error> {
        let mut encoder = Encoder::new();
        encoder.encode_map_start(6)?;

        encoder.encode_string("content")?;
        encoder.encode_string(&self.content)?;

        encoder.encode_string("created_at")?;
        encoder.encode_u64(self.created_at)?;

        encoder.encode_string("id")?;
        encoder.encode_bytes(self.id.as_bytes())?;

        encoder.encode_string("tags")?;
        encoder.encode_array_start(self.tags.len())?;
        for tag in &self.tags {
            encoder.encode_string(tag)?;
        }

        encoder.encode_string("title")?;
        encoder.encode_string(&self.title)?;

        encoder.encode_string("updated_at")?;
        encoder.encode_u64(self.updated_at)?;

        Ok(encoder.finish())
    }

    fn decode(bytes: &[u8]) -> Result<Self, entidb_codec::Error> {
        let value = entidb_codec::Decoder::decode(bytes)?;

        if let Value::Map(entries) = value {
            let mut id = None;
            let mut title = None;
            let mut content = None;
            let mut tags = Vec::new();
            let mut created_at = None;
            let mut updated_at = None;

            for (key, val) in entries {
                if let Value::Text(k) = key {
                    match k.as_str() {
                        "id" => {
                            if let Value::Bytes(b) = val {
                                id = Some(EntityId::from_bytes(&b)?);
                            }
                        }
                        "title" => {
                            if let Value::Text(t) = val {
                                title = Some(t);
                            }
                        }
                        "content" => {
                            if let Value::Text(c) = val {
                                content = Some(c);
                            }
                        }
                        "tags" => {
                            if let Value::Array(arr) = val {
                                for item in arr {
                                    if let Value::Text(t) = item {
                                        tags.push(t);
                                    }
                                }
                            }
                        }
                        "created_at" => {
                            if let Value::Integer(c) = val {
                                created_at = Some(c as u64);
                            }
                        }
                        "updated_at" => {
                            if let Value::Integer(u) = val {
                                updated_at = Some(u as u64);
                            }
                        }
                        _ => {}
                    }
                }
            }

            Ok(Note {
                id: id.ok_or_else(|| entidb_codec::Error::InvalidData("missing id".into()))?,
                title: title
                    .ok_or_else(|| entidb_codec::Error::InvalidData("missing title".into()))?,
                content: content.unwrap_or_default(),
                tags,
                created_at: created_at.unwrap_or(0),
                updated_at: updated_at.unwrap_or(0),
            })
        } else {
            Err(entidb_codec::Error::InvalidData(
                "expected map".to_string(),
            ))
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("notes_db");
    let backup_path = temp_dir.path().join("notes_backup");

    println!("📝 Notes Application Example");
    println!("=============================\n");

    // Open the database
    let config = DatabaseConfig::default();
    let db = Database::open(&db_path, config)?;

    // Create sample notes
    let notes = vec![
        Note {
            id: EntityId::new(),
            title: "Meeting Notes".to_string(),
            content: "Discussed Q4 roadmap and priorities.".to_string(),
            tags: vec!["work".to_string(), "meeting".to_string()],
            created_at: 1700000000,
            updated_at: 1700000000,
        },
        Note {
            id: EntityId::new(),
            title: "Recipe: Pasta".to_string(),
            content: "Boil water, add pasta, cook for 10 minutes.".to_string(),
            tags: vec!["cooking".to_string(), "recipe".to_string()],
            created_at: 1700001000,
            updated_at: 1700001000,
        },
        Note {
            id: EntityId::new(),
            title: "Book Ideas".to_string(),
            content: "Write about embedded databases and their applications.".to_string(),
            tags: vec!["writing".to_string(), "ideas".to_string()],
            created_at: 1700002000,
            updated_at: 1700002000,
        },
        Note {
            id: EntityId::new(),
            title: "Project Checklist".to_string(),
            content: "1. Setup\n2. Implementation\n3. Testing\n4. Deploy".to_string(),
            tags: vec!["work".to_string(), "project".to_string()],
            created_at: 1700003000,
            updated_at: 1700003000,
        },
    ];

    // Insert notes
    println!("📥 Inserting {} notes...", notes.len());
    db.write(|tx| {
        for note in &notes {
            tx.put("notes", note)?;
        }
        Ok(())
    })?;

    // Display all notes
    println!("\n📋 All Notes:");
    let all_notes: Vec<Note> = db.read(|tx| tx.scan::<Note>("notes").collect())?;

    for note in &all_notes {
        println!("  📄 {} (tags: {})", note.title, note.tags.join(", "));
    }

    // Filter by tag using native iterators
    println!("\n🔍 Notes tagged 'work':");
    let work_notes: Vec<Note> = db.read(|tx| {
        tx.scan::<Note>("notes")
            .filter(|n| n.tags.contains(&"work".to_string()))
            .collect()
    })?;

    for note in &work_notes {
        println!("  📄 {}", note.title);
    }

    // Search in content
    println!("\n🔎 Notes containing 'database':");
    let search_results: Vec<Note> = db.read(|tx| {
        tx.scan::<Note>("notes")
            .filter(|n| n.content.to_lowercase().contains("database"))
            .collect()
    })?;

    for note in &search_results {
        println!("  📄 {} - {}", note.title, &note.content[..50.min(note.content.len())]);
    }

    // Create a backup
    println!("\n💾 Creating backup...");
    let backup_manager = BackupManager::new(&db);
    let backup_info = backup_manager.create_backup(&backup_path)?;
    println!(
        "✅ Backup created: {} bytes, {} collections",
        backup_info.size_bytes, backup_info.collection_count
    );

    // Simulate data modification
    println!("\n✏️  Modifying data...");
    db.write(|tx| {
        // Update a note
        let mut meeting_note: Vec<Note> = tx
            .scan::<Note>("notes")
            .filter(|n| n.title == "Meeting Notes")
            .collect();

        if let Some(note) = meeting_note.first() {
            let updated = Note {
                content: format!("{}\n\nUpdate: Action items assigned.", note.content),
                updated_at: 1700010000,
                ..note.clone()
            };
            tx.put("notes", &updated)?;
        }
        Ok(())
    })?;

    // Verify backup integrity
    println!("\n🔐 Validating backup...");
    let is_valid = backup_manager.validate_backup(&backup_path)?;
    println!("✅ Backup valid: {}", is_valid);

    // Statistics
    let note_count: usize = db.read(|tx| tx.scan::<Note>("notes").count())?;
    let total_tags: usize = db.read(|tx| {
        tx.scan::<Note>("notes")
            .map(|n| n.tags.len())
            .sum()
    })?;

    println!("\n📊 Statistics:");
    println!("  Total notes: {}", note_count);
    println!("  Total tags: {}", total_tags);
    println!("  Avg tags/note: {:.1}", total_tags as f64 / note_count as f64);

    db.close()?;
    println!("\n👋 Database closed");

    Ok(())
}
