//! Basic EntiDB Example - Todo Application
//!
//! This example demonstrates core EntiDB functionality:
//! - Opening a database
//! - Defining entities with CBOR encoding
//! - CRUD operations within transactions
//! - Filtering using native Rust iterators
//!
//! Run with: cargo run -p rust_todo

use entidb_codec::{from_cbor, to_canonical_cbor, Value};
use entidb_core::{Database, EntityId};
use std::time::{SystemTime, UNIX_EPOCH};

/// A simple Todo item entity with CBOR encoding.
#[derive(Debug, Clone)]
struct Todo {
    id: EntityId,
    title: String,
    completed: bool,
    priority: u8,
    created_at: u64,
}

impl Todo {
    /// Creates a new Todo with a generated ID.
    fn new(title: &str, priority: u8) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            id: EntityId::new(),
            title: title.to_string(),
            completed: false,
            priority,
            created_at: now,
        }
    }

    /// Encodes the todo to canonical CBOR bytes.
    fn encode(&self) -> Vec<u8> {
        // Build a map with sorted keys for canonical CBOR
        let pairs = vec![
            (
                Value::Text("completed".to_string()),
                Value::Bool(self.completed),
            ),
            (
                Value::Text("created_at".to_string()),
                Value::Integer(self.created_at as i64),
            ),
            (
                Value::Text("id".to_string()),
                Value::Bytes(self.id.as_bytes().to_vec()),
            ),
            (
                Value::Text("priority".to_string()),
                Value::Integer(self.priority as i64),
            ),
            (
                Value::Text("title".to_string()),
                Value::Text(self.title.clone()),
            ),
        ];
        let value = Value::Map(pairs);
        to_canonical_cbor(&value).expect("encoding should succeed")
    }

    /// Decodes a todo from CBOR bytes.
    fn decode(id: EntityId, bytes: &[u8]) -> Result<Self, String> {
        let value = from_cbor(bytes).map_err(|e| e.to_string())?;

        if let Value::Map(entries) = value {
            let mut title = None;
            let mut completed = false;
            let mut priority = 0u8;
            let mut created_at = 0u64;

            for (key, val) in entries {
                if let Value::Text(k) = key {
                    match k.as_str() {
                        "title" => {
                            if let Value::Text(t) = val {
                                title = Some(t);
                            }
                        }
                        "completed" => {
                            if let Value::Bool(c) = val {
                                completed = c;
                            }
                        }
                        "priority" => {
                            if let Value::Integer(p) = val {
                                priority = p as u8;
                            }
                        }
                        "created_at" => {
                            if let Value::Integer(c) = val {
                                created_at = c as u64;
                            }
                        }
                        _ => {}
                    }
                }
            }

            Ok(Todo {
                id,
                title: title.ok_or("missing title")?,
                completed,
                priority,
                created_at,
            })
        } else {
            Err("expected CBOR map".to_string())
        }
    }

    /// Creates a copy with completed set to true.
    fn complete(self) -> Self {
        Self {
            completed: true,
            ..self
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Todo Application Example");
    println!("========================\n");

    // Open an in-memory database
    let db = Database::open_in_memory()?;
    println!("[OK] Database opened successfully");

    // Get the todos collection
    let todos_collection = db.collection("todos");

    // Create some todos
    let todos = vec![
        Todo::new("Learn EntiDB", 1),
        Todo::new("Build an app", 2),
        Todo {
            id: EntityId::new(),
            title: "Write tests".to_string(),
            completed: true,
            priority: 1,
            created_at: 1700000200,
        },
        Todo::new("Deploy to production", 3),
    ];

    // Insert todos in a transaction
    println!("\n[+] Inserting {} todos...", todos.len());
    db.transaction(|txn| {
        for todo in &todos {
            txn.put(todos_collection, todo.id, todo.encode())?;
        }
        Ok(())
    })?;
    println!("[OK] Todos inserted");

    // Read all todos using list()
    println!("\n[*] All todos:");
    let all_entries = db.list(todos_collection)?;
    let all_todos: Vec<Todo> = all_entries
        .iter()
        .filter_map(|(id, bytes)| Todo::decode(*id, bytes).ok())
        .collect();

    for todo in &all_todos {
        let status = if todo.completed { "✓" } else { "○" };
        println!("  {} [P{}] {}", status, todo.priority, todo.title);
    }

    // Filter incomplete high-priority todos using native Rust iterators
    println!("\n[!] High-priority incomplete todos:");
    let urgent: Vec<&Todo> = all_todos
        .iter()
        .filter(|t| !t.completed && t.priority == 1)
        .collect();

    for todo in &urgent {
        println!("  ○ {}", todo.title);
    }

    // Update a todo
    println!("\n[~] Completing 'Learn EntiDB'...");
    db.transaction(|txn| {
        if let Some(todo) = all_todos.iter().find(|t| t.title == "Learn EntiDB") {
            let updated = todo.clone().complete();
            txn.put(todos_collection, updated.id, updated.encode())?;
        }
        Ok(())
    })?;

    // Count completed vs incomplete
    let updated_entries = db.list(todos_collection)?;
    let updated_todos: Vec<Todo> = updated_entries
        .iter()
        .filter_map(|(id, bytes)| Todo::decode(*id, bytes).ok())
        .collect();

    let (completed, incomplete): (Vec<_>, Vec<_>) = updated_todos.iter().partition(|t| t.completed);

    println!("\n[#] Summary:");
    println!("  Completed: {}", completed.len());
    println!("  Incomplete: {}", incomplete.len());

    // Delete completed todos
    println!("\n[-] Deleting completed todos...");
    db.transaction(|txn| {
        for todo in &completed {
            txn.delete(todos_collection, todo.id)?;
        }
        Ok(())
    })?;

    let remaining = db.list(todos_collection)?;
    println!("[OK] Remaining todos: {}", remaining.len());

    // Close the database
    db.close()?;
    println!("\n[*] Database closed");

    Ok(())
}
