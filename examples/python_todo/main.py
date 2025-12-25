"""
EntiDB Python Example - Todo Application

This example demonstrates:
- Opening a database
- Basic CRUD operations
- Filtering with Python comprehensions (no SQL!)
- Transaction usage with context managers
- Iterator usage for memory efficiency

Run with: python main.py
"""

import json
import sys
import io
from dataclasses import dataclass
from typing import Optional
import time

# Ensure stdout can handle UTF-8 on Windows
if sys.platform == "win32":
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8', errors='replace')

# Note: entidb must be built first with `maturin develop`
import entidb


@dataclass
class Todo:
    """A simple todo item."""
    id: entidb.EntityId
    title: str
    completed: bool = False
    priority: int = 0
    created_at: int = 0

    @classmethod
    def create(cls, title: str, priority: int = 0) -> "Todo":
        """Creates a new todo with a generated ID."""
        return cls(
            id=entidb.EntityId(),
            title=title,
            priority=priority,
            created_at=int(time.time()),
        )

    def to_bytes(self) -> bytes:
        """Convert to JSON bytes for storage."""
        data = {
            "title": self.title,
            "completed": self.completed,
            "priority": self.priority,
            "created_at": self.created_at,
        }
        return json.dumps(data).encode("utf-8")

    @classmethod
    def from_bytes(cls, entity_id: entidb.EntityId, data: bytes) -> "Todo":
        """Create from JSON bytes."""
        obj = json.loads(data.decode("utf-8"))
        return cls(
            id=entity_id,
            title=obj.get("title", ""),
            completed=obj.get("completed", False),
            priority=obj.get("priority", 0),
            created_at=obj.get("created_at", 0),
        )

    def complete(self) -> "Todo":
        """Returns a copy with completed=True."""
        return Todo(
            id=self.id,
            title=self.title,
            completed=True,
            priority=self.priority,
            created_at=self.created_at,
        )

    def __str__(self) -> str:
        status = "✓" if self.completed else "○"
        return f"{status} [P{self.priority}] {self.title}"


def main():
    print("📁 Creating in-memory database")

    # Open an in-memory database using context manager
    with entidb.Database.open_memory() as db:
        print("✅ Database opened successfully")
        print(f"   Version: {entidb.version()}")

        # Get the todos collection
        todos_collection = db.collection("todos")
        print(f"   Collection: {todos_collection.name} (id={todos_collection.id})")

        # Create some todos
        todos = [
            Todo.create("Learn EntiDB", priority=1),
            Todo.create("Build an app", priority=2),
            Todo(
                id=entidb.EntityId(),
                title="Write tests",
                completed=True,
                priority=1,
                created_at=1700000200,
            ),
            Todo.create("Deploy to production", priority=3),
        ]

        # Insert todos using transaction context manager (auto-commits!)
        print(f"\n📝 Inserting {len(todos)} todos...")
        with db.transaction() as txn:
            for todo in todos:
                txn.put(todos_collection, todo.id, todo.to_bytes())
        print("✅ Todos inserted (auto-committed)")

        # Read all todos using list()
        print("\n📋 All todos:")
        all_todos = [
            Todo.from_bytes(entity_id, data)
            for entity_id, data in db.list(todos_collection)
        ]

        for todo in all_todos:
            print(f"  {todo}")

        # Filter incomplete high-priority todos using Python comprehensions (NO SQL!)
        print("\n⚡ High-priority incomplete todos:")
        urgent = [t for t in all_todos if not t.completed and t.priority == 1]

        for todo in urgent:
            print(f"  ○ {todo.title}")

        # Demonstrate iterator usage
        print("\n🔄 Using iterator:")
        iterator = db.iter(todos_collection)
        print(f"  Total items: {iterator.count()}")

        for entity_id, data in iterator:
            todo = Todo.from_bytes(entity_id, data)
            hex_id = entity_id.to_hex()[:8]
            print(f"  {todo.title} (id: {hex_id}...)")

        # Update a todo using context manager
        print("\n✏️  Completing 'Learn EntiDB'...")
        with db.transaction() as txn:
            for todo in all_todos:
                if todo.title == "Learn EntiDB":
                    txn.put(todos_collection, todo.id, todo.complete().to_bytes())
                    break

        # Count completed vs incomplete
        updated_todos = [
            Todo.from_bytes(entity_id, data)
            for entity_id, data in db.list(todos_collection)
        ]
        completed = [t for t in updated_todos if t.completed]
        incomplete = [t for t in updated_todos if not t.completed]

        print("\n📊 Summary:")
        print(f"  Completed: {len(completed)}")
        print(f"  Incomplete: {len(incomplete)}")
        print(f"  Total count: {db.count(todos_collection)}")

        # Demonstrate abort on exception
        print("\n🔄 Demonstrating transaction abort on exception...")
        try:
            with db.transaction() as txn:
                txn.put(todos_collection, entidb.EntityId(), b"temp")
                raise ValueError("Simulated error!")
        except ValueError:
            print("  Transaction aborted due to exception")

        # Verify count unchanged
        print(f"  Count still: {db.count(todos_collection)}")

        # Delete completed todos
        print("\n🗑️  Deleting completed todos...")
        with db.transaction() as txn:
            for todo in completed:
                txn.delete(todos_collection, todo.id)

        remaining = db.count(todos_collection)
        print(f"✅ Remaining todos: {remaining}")

    # Database auto-closed by context manager
    print("\n👋 Database closed (auto by context manager)")


if __name__ == "__main__":
    main()
