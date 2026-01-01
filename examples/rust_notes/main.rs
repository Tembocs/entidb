//! Notes Application Example
//!
//! This example demonstrates:
//! - Working with complex entities (arrays, nested structures)
//! - Advanced filtering with Rust iterators
//! - Entity updates within transactions
//!
//! Run with: cargo run -p rust_notes

use entidb_codec::{from_cbor, to_canonical_cbor, Value};
use entidb_core::{Database, EntityId};
use std::time::{SystemTime, UNIX_EPOCH};

/// A note with tags and content.
#[derive(Debug, Clone)]
struct Note {
    id: EntityId,
    title: String,
    content: String,
    tags: Vec<String>,
    created_at: u64,
    updated_at: u64,
}

impl Note {
    /// Creates a new note with a generated ID.
    fn new(title: &str, content: &str, tags: Vec<&str>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            id: EntityId::new(),
            title: title.to_string(),
            content: content.to_string(),
            tags: tags.into_iter().map(String::from).collect(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Encodes the note to canonical CBOR bytes.
    fn encode(&self) -> Vec<u8> {
        // Build tags array
        let tags_array: Vec<Value> = self.tags.iter().map(|t| Value::Text(t.clone())).collect();

        // Build a map with sorted keys for canonical CBOR
        let pairs = vec![
            (
                Value::Text("content".to_string()),
                Value::Text(self.content.clone()),
            ),
            (
                Value::Text("created_at".to_string()),
                Value::Integer(self.created_at as i64),
            ),
            (
                Value::Text("id".to_string()),
                Value::Bytes(self.id.as_bytes().to_vec()),
            ),
            (Value::Text("tags".to_string()), Value::Array(tags_array)),
            (
                Value::Text("title".to_string()),
                Value::Text(self.title.clone()),
            ),
            (
                Value::Text("updated_at".to_string()),
                Value::Integer(self.updated_at as i64),
            ),
        ];
        let value = Value::Map(pairs);
        to_canonical_cbor(&value).expect("encoding should succeed")
    }

    /// Decodes a note from CBOR bytes.
    fn decode(id: EntityId, bytes: &[u8]) -> Result<Self, String> {
        let value = from_cbor(bytes).map_err(|e| e.to_string())?;

        if let Value::Map(entries) = value {
            let mut title = None;
            let mut content = String::new();
            let mut tags = Vec::new();
            let mut created_at = 0u64;
            let mut updated_at = 0u64;

            for (key, val) in entries {
                if let Value::Text(k) = key {
                    match k.as_str() {
                        "title" => {
                            if let Value::Text(t) = val {
                                title = Some(t);
                            }
                        }
                        "content" => {
                            if let Value::Text(c) = val {
                                content = c;
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
                                created_at = c as u64;
                            }
                        }
                        "updated_at" => {
                            if let Value::Integer(u) = val {
                                updated_at = u as u64;
                            }
                        }
                        _ => {}
                    }
                }
            }

            Ok(Note {
                id,
                title: title.ok_or("missing title")?,
                content,
                tags,
                created_at,
                updated_at,
            })
        } else {
            Err("expected CBOR map".to_string())
        }
    }

    /// Updates the content and timestamp.
    fn update_content(self, new_content: &str) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            content: new_content.to_string(),
            updated_at: now,
            ..self
        }
    }

    /// Checks if the note has a specific tag.
    fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Notes Application Example");
    println!("=========================\n");

    // Open an in-memory database
    let db = Database::open_in_memory()?;

    // Get the notes collection
    let notes_collection = db.collection("notes");

    // Create sample notes
    let notes = vec![
        Note::new(
            "Meeting Notes",
            "Discussed Q4 roadmap and priorities.",
            vec!["work", "meeting"],
        ),
        Note::new(
            "Recipe: Pasta",
            "Boil water, add pasta, cook for 10 minutes.",
            vec!["cooking", "recipe"],
        ),
        Note::new(
            "Book Ideas",
            "Write about embedded databases and their applications.",
            vec!["writing", "ideas"],
        ),
        Note::new(
            "Project Checklist",
            "1. Setup\n2. Implementation\n3. Testing\n4. Deploy",
            vec!["work", "project"],
        ),
    ];

    // Insert notes in a transaction
    println!("[+] Inserting {} notes...", notes.len());
    db.transaction(|txn| {
        for note in &notes {
            txn.put(notes_collection, note.id, note.encode())?;
        }
        Ok(())
    })?;

    // Display all notes
    println!("\n[*] All Notes:");
    let all_entries = db.list(notes_collection)?;
    let all_notes: Vec<Note> = all_entries
        .iter()
        .filter_map(|(id, bytes)| Note::decode(*id, bytes).ok())
        .collect();

    for note in &all_notes {
        println!("  - {} (tags: {})", note.title, note.tags.join(", "));
    }

    // Filter by tag using native Rust iterators
    println!("\n[?] Notes tagged 'work':");
    let work_notes: Vec<&Note> = all_notes.iter().filter(|n| n.has_tag("work")).collect();

    for note in &work_notes {
        println!("  - {}", note.title);
    }

    // Search in content
    println!("\n[?] Notes containing 'database':");
    let search_results: Vec<&Note> = all_notes
        .iter()
        .filter(|n| n.content.to_lowercase().contains("database"))
        .collect();

    for note in &search_results {
        let preview = &note.content[..50.min(note.content.len())];
        println!("  - {} - {}", note.title, preview);
    }

    // Update a note
    println!("\n[~] Updating 'Meeting Notes'...");
    db.transaction(|txn| {
        if let Some(note) = all_notes.iter().find(|n| n.title == "Meeting Notes") {
            let new_content = format!("{}\n\nUpdate: Action items assigned.", note.content);
            let updated = note.clone().update_content(&new_content);
            txn.put(notes_collection, updated.id, updated.encode())?;
        }
        Ok(())
    })?;

    // Verify the update
    let updated_entries = db.list(notes_collection)?;
    let updated_notes: Vec<Note> = updated_entries
        .iter()
        .filter_map(|(id, bytes)| Note::decode(*id, bytes).ok())
        .collect();

    if let Some(meeting_note) = updated_notes.iter().find(|n| n.title == "Meeting Notes") {
        println!("  Updated content: {}", meeting_note.content);
    }

    // Statistics using iterators
    let total_tags: usize = all_notes.iter().map(|n| n.tags.len()).sum();

    println!("\n[#] Statistics:");
    println!("  Total notes: {}", all_notes.len());
    println!("  Total tags: {}", total_tags);
    println!(
        "  Avg tags/note: {:.1}",
        total_tags as f64 / all_notes.len() as f64
    );

    // Find notes with multiple tags
    let multi_tagged: Vec<&Note> = all_notes.iter().filter(|n| n.tags.len() > 1).collect();
    println!("  Notes with multiple tags: {}", multi_tagged.len());

    // Delete notes by tag
    println!("\n[-] Deleting 'cooking' notes...");
    db.transaction(|txn| {
        let to_delete: Vec<EntityId> = all_notes
            .iter()
            .filter(|n| n.has_tag("cooking"))
            .map(|n| n.id)
            .collect();

        for id in to_delete {
            txn.delete(notes_collection, id)?;
        }
        Ok(())
    })?;

    let remaining = db.list(notes_collection)?;
    println!("[OK] Remaining notes: {}", remaining.len());

    db.close()?;
    println!("\n[*] Database closed");

    Ok(())
}
