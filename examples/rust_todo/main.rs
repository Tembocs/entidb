//! Basic EntiDB Example - Todo Application
//!
//! This example demonstrates core EntiDB functionality:
//! - Opening a database
//! - Defining entities
//! - CRUD operations within transactions
//! - Filtering using native Rust iterators

use entidb_codec::{Encoder, Value};
use entidb_core::{
    collection::Collection,
    database::{Database, DatabaseConfig},
    entity::{Entity, EntityCodec, EntityId},
    error::Error,
    index::BTreeIndexDef,
    transaction::Transaction,
};
use entidb_storage::file::FileBackend;
use std::path::PathBuf;
use tempfile::TempDir;

/// A simple Todo item entity
#[derive(Debug, Clone)]
struct Todo {
    id: EntityId,
    title: String,
    completed: bool,
    priority: u8,
    created_at: u64,
}

impl Entity for Todo {
    fn id(&self) -> EntityId {
        self.id
    }
}

impl EntityCodec for Todo {
    fn encode(&self) -> Result<Vec<u8>, entidb_codec::Error> {
        let mut encoder = Encoder::new();
        encoder.encode_map_start(5)?;

        // Encode in canonical order (sorted by key)
        encoder.encode_string("completed")?;
        encoder.encode_bool(self.completed)?;

        encoder.encode_string("created_at")?;
        encoder.encode_u64(self.created_at)?;

        encoder.encode_string("id")?;
        encoder.encode_bytes(self.id.as_bytes())?;

        encoder.encode_string("priority")?;
        encoder.encode_u64(self.priority as u64)?;

        encoder.encode_string("title")?;
        encoder.encode_string(&self.title)?;

        Ok(encoder.finish())
    }

    fn decode(bytes: &[u8]) -> Result<Self, entidb_codec::Error> {
        let value = entidb_codec::Decoder::decode(bytes)?;

        if let Value::Map(entries) = value {
            let mut id = None;
            let mut title = None;
            let mut completed = None;
            let mut priority = None;
            let mut created_at = None;

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
                        "completed" => {
                            if let Value::Bool(c) = val {
                                completed = Some(c);
                            }
                        }
                        "priority" => {
                            if let Value::Integer(p) = val {
                                priority = Some(p as u8);
                            }
                        }
                        "created_at" => {
                            if let Value::Integer(c) = val {
                                created_at = Some(c as u64);
                            }
                        }
                        _ => {}
                    }
                }
            }

            Ok(Todo {
                id: id.ok_or_else(|| entidb_codec::Error::InvalidData("missing id".into()))?,
                title: title
                    .ok_or_else(|| entidb_codec::Error::InvalidData("missing title".into()))?,
                completed: completed.unwrap_or(false),
                priority: priority.unwrap_or(0),
                created_at: created_at.unwrap_or(0),
            })
        } else {
            Err(entidb_codec::Error::InvalidData(
                "expected map".to_string(),
            ))
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary directory for the database
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("todo_db");

    println!("📁 Creating database at: {:?}", db_path);

    // Open the database
    let config = DatabaseConfig::default();
    let db = Database::open(&db_path, config)?;

    println!("✅ Database opened successfully");

    // Create some todos
    let todos = vec![
        Todo {
            id: EntityId::new(),
            title: "Learn EntiDB".to_string(),
            completed: false,
            priority: 1,
            created_at: 1700000000,
        },
        Todo {
            id: EntityId::new(),
            title: "Build an app".to_string(),
            completed: false,
            priority: 2,
            created_at: 1700000100,
        },
        Todo {
            id: EntityId::new(),
            title: "Write tests".to_string(),
            completed: true,
            priority: 1,
            created_at: 1700000200,
        },
        Todo {
            id: EntityId::new(),
            title: "Deploy to production".to_string(),
            completed: false,
            priority: 3,
            created_at: 1700000300,
        },
    ];

    // Insert todos in a transaction
    println!("\n📝 Inserting {} todos...", todos.len());
    db.write(|tx| {
        for todo in &todos {
            tx.put("todos", todo)?;
        }
        Ok(())
    })?;
    println!("✅ Todos inserted");

    // Read all todos
    println!("\n📋 All todos:");
    let all_todos: Vec<Todo> = db.read(|tx| tx.scan::<Todo>("todos").collect())?;

    for todo in &all_todos {
        let status = if todo.completed { "✓" } else { "○" };
        println!(
            "  {} [P{}] {}",
            status, todo.priority, todo.title
        );
    }

    // Filter incomplete high-priority todos using native Rust iterators
    println!("\n⚡ High-priority incomplete todos:");
    let urgent: Vec<Todo> = db.read(|tx| {
        tx.scan::<Todo>("todos")
            .filter(|t| !t.completed && t.priority == 1)
            .collect()
    })?;

    for todo in &urgent {
        println!("  ○ {}", todo.title);
    }

    // Update a todo
    println!("\n✏️  Completing 'Learn EntiDB'...");
    db.write(|tx| {
        let mut updated: Vec<Todo> = tx
            .scan::<Todo>("todos")
            .filter(|t| t.title == "Learn EntiDB")
            .collect();

        if let Some(todo) = updated.first_mut() {
            let completed_todo = Todo {
                completed: true,
                ..todo.clone()
            };
            tx.put("todos", &completed_todo)?;
        }
        Ok(())
    })?;

    // Count completed vs incomplete
    let (completed, incomplete): (Vec<_>, Vec<_>) = db.read(|tx| {
        tx.scan::<Todo>("todos")
            .partition(|t| t.completed)
    })?;

    println!("\n📊 Summary:");
    println!("  Completed: {}", completed.len());
    println!("  Incomplete: {}", incomplete.len());

    // Delete completed todos
    println!("\n🗑️  Deleting completed todos...");
    db.write(|tx| {
        let to_delete: Vec<EntityId> = tx
            .scan::<Todo>("todos")
            .filter(|t| t.completed)
            .map(|t| t.id)
            .collect();

        for id in to_delete {
            tx.delete("todos", id)?;
        }
        Ok(())
    })?;

    let remaining: Vec<Todo> = db.read(|tx| tx.scan::<Todo>("todos").collect())?;
    println!("✅ Remaining todos: {}", remaining.len());

    // Close the database
    db.close()?;
    println!("\n👋 Database closed");

    Ok(())
}
