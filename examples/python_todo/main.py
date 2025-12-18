"""
EntiDB Python Example - Todo Application

This example demonstrates:
- Opening a database
- Basic CRUD operations
- Filtering with Python comprehensions
- Transaction usage
"""

import entidb
from dataclasses import dataclass
from typing import List, Optional
import tempfile
import os


@dataclass
class Todo:
    """A simple todo item."""
    id: str
    title: str
    completed: bool = False
    priority: int = 0
    created_at: int = 0


def main():
    # Create a temporary directory for the database
    with tempfile.TemporaryDirectory() as temp_dir:
        db_path = os.path.join(temp_dir, "todo_db")
        
        print("📁 Creating database at:", db_path)
        
        # Open the database
        db = entidb.Database.open(db_path)
        print("✅ Database opened successfully")
        
        # Create some todos
        todos = [
            Todo(
                id=entidb.EntityId.new(),
                title="Learn EntiDB",
                completed=False,
                priority=1,
                created_at=1700000000,
            ),
            Todo(
                id=entidb.EntityId.new(),
                title="Build an app",
                completed=False,
                priority=2,
                created_at=1700000100,
            ),
            Todo(
                id=entidb.EntityId.new(),
                title="Write tests",
                completed=True,
                priority=1,
                created_at=1700000200,
            ),
            Todo(
                id=entidb.EntityId.new(),
                title="Deploy to production",
                completed=False,
                priority=3,
                created_at=1700000300,
            ),
        ]
        
        # Insert todos in a transaction
        print(f"\n📝 Inserting {len(todos)} todos...")
        with db.transaction() as tx:
            for todo in todos:
                tx.put("todos", todo_to_dict(todo))
        print("✅ Todos inserted")
        
        # Read all todos
        print("\n📋 All todos:")
        all_todos = list(db.scan("todos"))
        
        for data in all_todos:
            todo = dict_to_todo(data)
            status = "✓" if todo.completed else "○"
            print(f"  {status} [P{todo.priority}] {todo.title}")
        
        # Filter incomplete high-priority todos using Python comprehensions
        print("\n⚡ High-priority incomplete todos:")
        urgent = [
            dict_to_todo(t) for t in db.scan("todos")
            if not t["completed"] and t["priority"] == 1
        ]
        
        for todo in urgent:
            print(f"  ○ {todo.title}")
        
        # Update a todo
        print("\n✏️  Completing 'Learn EntiDB'...")
        with db.transaction() as tx:
            for data in tx.scan("todos"):
                if data["title"] == "Learn EntiDB":
                    data["completed"] = True
                    tx.put("todos", data)
                    break
        
        # Count completed vs incomplete using generators
        all_data = list(db.scan("todos"))
        completed = [t for t in all_data if t["completed"]]
        incomplete = [t for t in all_data if not t["completed"]]
        
        print("\n📊 Summary:")
        print(f"  Completed: {len(completed)}")
        print(f"  Incomplete: {len(incomplete)}")
        
        # Delete completed todos
        print("\n🗑️  Deleting completed todos...")
        with db.transaction() as tx:
            to_delete = [t["id"] for t in tx.scan("todos") if t["completed"]]
            for entity_id in to_delete:
                tx.delete("todos", entity_id)
        
        remaining = list(db.scan("todos"))
        print(f"✅ Remaining todos: {len(remaining)}")
        
        # Close the database
        db.close()
        print("\n👋 Database closed")


def todo_to_dict(todo: Todo) -> dict:
    """Convert a Todo to a dictionary for storage."""
    return {
        "id": todo.id,
        "title": todo.title,
        "completed": todo.completed,
        "priority": todo.priority,
        "created_at": todo.created_at,
    }


def dict_to_todo(data: dict) -> Todo:
    """Convert a dictionary back to a Todo."""
    return Todo(
        id=data["id"],
        title=data["title"],
        completed=data.get("completed", False),
        priority=data.get("priority", 0),
        created_at=data.get("created_at", 0),
    )


if __name__ == "__main__":
    main()
