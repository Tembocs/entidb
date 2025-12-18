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


@dataclass
class Todo:
    """A simple todo item."""
    id: entidb.EntityId
    title: str
    completed: bool = False
    priority: int = 0
    created_at: int = 0


def todo_to_bytes(todo: Todo) -> bytes:
    """Convert a Todo to bytes for storage."""
    return f"{todo.title}|{todo.completed}|{todo.priority}|{todo.created_at}".encode()


def todo_from_bytes(entity_id: entidb.EntityId, data: bytes) -> Todo:
    """Convert bytes back to a Todo."""
    parts = data.decode().split("|")
    return Todo(
        id=entity_id,
        title=parts[0] if len(parts) > 0 else "",
        completed=parts[1] == "True" if len(parts) > 1 else False,
        priority=int(parts[2]) if len(parts) > 2 else 0,
        created_at=int(parts[3]) if len(parts) > 3 else 0,
    )


def main():
    print("📁 Creating in-memory database")

    # Open an in-memory database
    db = entidb.Database.open_memory()
    print("✅ Database opened successfully")

    # Get the todos collection
    todos_collection = db.collection("todos")

    # Create some todos
    todos = [
        Todo(
            id=entidb.EntityId(),
            title="Learn EntiDB",
            completed=False,
            priority=1,
            created_at=1700000000,
        ),
        Todo(
            id=entidb.EntityId(),
            title="Build an app",
            completed=False,
            priority=2,
            created_at=1700000100,
        ),
        Todo(
            id=entidb.EntityId(),
            title="Write tests",
            completed=True,
            priority=1,
            created_at=1700000200,
        ),
        Todo(
            id=entidb.EntityId(),
            title="Deploy to production",
            completed=False,
            priority=3,
            created_at=1700000300,
        ),
    ]

    # Insert todos in a transaction
    print(f"\n📝 Inserting {len(todos)} todos...")
    txn = db.transaction()
    for todo in todos:
        txn.put(todos_collection, todo.id, todo_to_bytes(todo))
    db.commit(txn)
    print("✅ Todos inserted")

    # Read all todos using list()
    print("\n📋 All todos:")
    all_todos = [
        todo_from_bytes(entity_id, data)
        for entity_id, data in db.list(todos_collection)
    ]

    for todo in all_todos:
        status = "✓" if todo.completed else "○"
        print(f"  {status} [P{todo.priority}] {todo.title}")

    # Filter incomplete high-priority todos using Python comprehensions
    print("\n⚡ High-priority incomplete todos:")
    urgent = [t for t in all_todos if not t.completed and t.priority == 1]

    for todo in urgent:
        print(f"  ○ {todo.title}")

    # Update a todo
    print("\n✏️  Completing 'Learn EntiDB'...")
    txn = db.transaction()
    for todo in all_todos:
        if todo.title == "Learn EntiDB":
            updated = Todo(
                id=todo.id,
                title=todo.title,
                completed=True,
                priority=todo.priority,
                created_at=todo.created_at,
            )
            txn.put(todos_collection, todo.id, todo_to_bytes(updated))
            break
    db.commit(txn)

    # Count completed vs incomplete
    updated_todos = [
        todo_from_bytes(entity_id, data)
        for entity_id, data in db.list(todos_collection)
    ]
    completed = [t for t in updated_todos if t.completed]
    incomplete = [t for t in updated_todos if not t.completed]

    print("\n📊 Summary:")
    print(f"  Completed: {len(completed)}")
    print(f"  Incomplete: {len(incomplete)}")

    # Delete completed todos
    print("\n🗑️  Deleting completed todos...")
    txn = db.transaction()
    for todo in completed:
        txn.delete(todos_collection, todo.id)
    db.commit(txn)

    remaining = db.count(todos_collection)
    print(f"✅ Remaining todos: {remaining}")

    # Close the database
    db.close()
    print("\n👋 Database closed")


if __name__ == "__main__":
    main()
