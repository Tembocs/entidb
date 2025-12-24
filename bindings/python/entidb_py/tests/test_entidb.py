"""Tests for EntiDB Python bindings."""

import pytest

# Note: Tests require the native library to be built.
# Run: maturin develop

try:
    from entidb import Database, EntityId, Collection, Transaction, EntityIterator, version
    ENTIDB_AVAILABLE = True
except ImportError:
    ENTIDB_AVAILABLE = False


@pytest.mark.skipif(not ENTIDB_AVAILABLE, reason="entidb not built")
class TestVersion:
    def test_version(self):
        ver = version()
        assert isinstance(ver, str)
        assert len(ver) > 0


@pytest.mark.skipif(not ENTIDB_AVAILABLE, reason="entidb not built")
class TestEntityId:
    def test_create(self):
        id1 = EntityId()
        id2 = EntityId()
        assert id1 != id2

    def test_from_bytes(self):
        data = bytes([1] * 16)
        entity_id = EntityId.from_bytes(data)
        assert entity_id.to_bytes() == data

    def test_from_bytes_wrong_size(self):
        with pytest.raises(ValueError):
            EntityId.from_bytes(b"short")

    def test_to_hex(self):
        entity_id = EntityId.from_bytes(bytes([0x01, 0x02] + [0] * 14))
        hex_str = entity_id.to_hex()
        assert hex_str.startswith("0102")

    def test_equality(self):
        data = bytes([42] * 16)
        id1 = EntityId.from_bytes(data)
        id2 = EntityId.from_bytes(data)
        assert id1 == id2

    def test_hash(self):
        id1 = EntityId()
        id2 = EntityId()
        # Different IDs should (usually) have different hashes
        # Same ID should have same hash
        id3 = EntityId.from_bytes(id1.to_bytes())
        assert hash(id1) == hash(id3)


@pytest.mark.skipif(not ENTIDB_AVAILABLE, reason="entidb not built")
class TestDatabase:
    def test_open_memory(self):
        db = Database.open_memory()
        assert db.is_open
        db.close()
        assert not db.is_open

    def test_context_manager(self):
        with Database.open_memory() as db:
            assert db.is_open
        # Database should be closed after context

    def test_collection(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            assert users.name == "users"
            assert users.id >= 0

    def test_put_and_get(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()
            data = b"hello world"

            db.put(users, entity_id, data)
            result = db.get(users, entity_id)

            assert result == data

    def test_get_missing(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()

            result = db.get(users, entity_id)
            assert result is None

    def test_delete(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()

            db.put(users, entity_id, b"data")
            assert db.get(users, entity_id) is not None

            db.delete(users, entity_id)
            assert db.get(users, entity_id) is None

    def test_list(self):
        with Database.open_memory() as db:
            users = db.collection("users")

            ids = [EntityId() for _ in range(3)]
            for i, entity_id in enumerate(ids):
                db.put(users, entity_id, f"data-{i}".encode())

            entities = db.list(users)
            assert len(entities) == 3

    def test_count(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            assert db.count(users) == 0

            for i in range(5):
                db.put(users, EntityId(), f"data-{i}".encode())

            assert db.count(users) == 5


@pytest.mark.skipif(not ENTIDB_AVAILABLE, reason="entidb not built")
class TestTransaction:
    def test_commit(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()

            txn = db.transaction()
            txn.put(users, entity_id, b"txn data")
            txn.commit()

            assert db.get(users, entity_id) == b"txn data"

    def test_uncommitted_not_visible(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()

            txn = db.transaction()
            txn.put(users, entity_id, b"data")

            # Without commit, data is not visible outside transaction
            assert db.get(users, entity_id) is None

    def test_transaction_sees_own_writes(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()

            txn = db.transaction()
            txn.put(users, entity_id, b"uncommitted")

            # Transaction should see its own writes
            result = txn.get(users, entity_id)
            assert result == b"uncommitted"

    def test_multiple_operations(self):
        with Database.open_memory() as db:
            users = db.collection("users")

            ids = [EntityId() for _ in range(3)]
            txn = db.transaction()
            for i, entity_id in enumerate(ids):
                txn.put(users, entity_id, f"data-{i}".encode())
            txn.commit()

            assert db.count(users) == 3

    def test_delete_in_transaction(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()

            # First put outside transaction
            db.put(users, entity_id, b"original")

            # Delete in transaction
            txn = db.transaction()
            txn.delete(users, entity_id)

            # Transaction should see the delete
            assert txn.get(users, entity_id) is None

            txn.commit()

            # After commit, should be deleted
            assert db.get(users, entity_id) is None

    def test_abort(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()

            txn = db.transaction()
            txn.put(users, entity_id, b"aborted data")
            txn.abort()

            # Aborted transaction should not persist
            assert db.get(users, entity_id) is None

    def test_context_manager_commit(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()

            with db.transaction() as txn:
                txn.put(users, entity_id, b"context data")

            # Should be committed after context exit
            assert db.get(users, entity_id) == b"context data"

    def test_context_manager_abort_on_exception(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()

            try:
                with db.transaction() as txn:
                    txn.put(users, entity_id, b"error data")
                    raise ValueError("simulated error")
            except ValueError:
                pass

            # Should not be committed due to exception
            assert db.get(users, entity_id) is None


@pytest.mark.skipif(not ENTIDB_AVAILABLE, reason="entidb not built")
class TestEntityIterator:
    def test_iter(self):
        with Database.open_memory() as db:
            users = db.collection("users")

            ids = [EntityId() for _ in range(3)]
            for i, entity_id in enumerate(ids):
                db.put(users, entity_id, f"data-{i}".encode())

            iterator = db.iter(users)
            count = 0
            for entity_id, data in iterator:
                count += 1
                assert isinstance(entity_id, EntityId)
                assert isinstance(data, bytes)

            assert count == 3

    def test_remaining(self):
        with Database.open_memory() as db:
            users = db.collection("users")

            for i in range(5):
                db.put(users, EntityId(), f"data-{i}".encode())

            iterator = db.iter(users)
            assert iterator.remaining() == 5
            assert iterator.count() == 5

            next(iterator)
            assert iterator.remaining() == 4

    def test_empty_collection(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            iterator = db.iter(users)

            items = list(iterator)
            assert items == []
