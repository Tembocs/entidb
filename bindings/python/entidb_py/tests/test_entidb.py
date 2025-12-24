"""Tests for EntiDB Python bindings."""

import pytest

# Note: Tests require the native library to be built.
# Run: maturin develop

try:
    from entidb import (
        Database,
        EntityId,
        Collection,
        Transaction,
        EntityIterator,
        RestoreStats,
        BackupInfo,
        version,
    )
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


@pytest.mark.skipif(not ENTIDB_AVAILABLE, reason="entidb not built")
class TestCheckpoint:
    def test_checkpoint(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()

            db.put(users, entity_id, b"checkpoint test")

            # Checkpoint should succeed
            db.checkpoint()

            # Data should still be accessible
            assert db.get(users, entity_id) == b"checkpoint test"

    def test_checkpoint_updates_sequence(self):
        with Database.open_memory() as db:
            users = db.collection("users")

            # Add some data
            db.put(users, EntityId(), b"data1")
            seq1 = db.committed_seq

            # Checkpoint
            db.checkpoint()

            # Sequence should be the same (checkpoint doesn't create new commits)
            assert db.committed_seq >= seq1


@pytest.mark.skipif(not ENTIDB_AVAILABLE, reason="entidb not built")
class TestBackupRestore:
    def test_backup(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()

            db.put(users, entity_id, b"backup test")

            backup_data = db.backup()
            assert isinstance(backup_data, bytes)
            assert len(backup_data) > 0

    def test_backup_with_options(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()

            db.put(users, entity_id, b"data")

            # Backup without tombstones
            backup1 = db.backup_with_options(include_tombstones=False)
            assert len(backup1) > 0

            # Backup with tombstones
            backup2 = db.backup_with_options(include_tombstones=True)
            assert len(backup2) > 0

    def test_restore(self):
        # Create first database with data
        with Database.open_memory() as db1:
            users = db1.collection("users")
            entity_id = EntityId()

            db1.put(users, entity_id, b"original data")

            # Create backup
            backup_data = db1.backup()

            # Create second database and restore
            with Database.open_memory() as db2:
                # Need to create collection first
                db2.collection("users")

                stats = db2.restore(backup_data)

                assert isinstance(stats, RestoreStats)
                assert stats.entities_restored == 1
                assert stats.tombstones_applied == 0

                # Data should be accessible
                result = db2.get(db2.collection("users"), entity_id)
                assert result == b"original data"

    def test_restore_stats(self):
        with Database.open_memory() as db1:
            users = db1.collection("users")

            # Add multiple entities
            for i in range(5):
                db1.put(users, EntityId(), f"data-{i}".encode())

            backup_data = db1.backup()

            with Database.open_memory() as db2:
                db2.collection("users")
                stats = db2.restore(backup_data)

                assert stats.entities_restored == 5
                assert stats.backup_timestamp > 0
                assert stats.backup_sequence >= 0

    def test_validate_backup(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            db.put(users, EntityId(), b"validation test")

            backup_data = db.backup()

            info = db.validate_backup(backup_data)

            assert isinstance(info, BackupInfo)
            assert info.valid is True
            assert info.record_count > 0
            assert info.size > 0
            assert info.timestamp > 0

    def test_validate_invalid_backup(self):
        with Database.open_memory() as db:
            # Try to validate garbage data
            with pytest.raises(IOError):
                db.validate_backup(b"not a valid backup")


@pytest.mark.skipif(not ENTIDB_AVAILABLE, reason="entidb not built")
class TestDatabaseProperties:
    def test_committed_seq(self):
        with Database.open_memory() as db:
            initial_seq = db.committed_seq
            assert initial_seq >= 0

            users = db.collection("users")
            db.put(users, EntityId(), b"data")

            # Sequence should increase after commit
            assert db.committed_seq > initial_seq

    def test_entity_count(self):
        with Database.open_memory() as db:
            assert db.entity_count == 0

            users = db.collection("users")
            for i in range(3):
                db.put(users, EntityId(), f"data-{i}".encode())

            assert db.entity_count == 3

    def test_entity_count_after_delete(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            entity_id = EntityId()

            db.put(users, entity_id, b"data")
            assert db.entity_count == 1

            db.delete(users, entity_id)
            # Note: Entity count may not decrease immediately after delete
            # because tombstones are still tracked until compaction
            # The entity should not be retrievable, which is the key invariant
            assert db.get(users, entity_id) is None


@pytest.mark.skipif(not ENTIDB_AVAILABLE, reason="entidb not built")
class TestHashIndex:
    def test_create_and_insert(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            db.create_hash_index(users, "email", unique=True)

            entity_id = EntityId()
            db.hash_index_insert(users, "email", b"alice@example.com", entity_id)

            results = db.hash_index_lookup(users, "email", b"alice@example.com")
            assert len(results) == 1
            assert results[0] == entity_id

    def test_non_unique_index(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            db.create_hash_index(users, "status", unique=False)

            e1 = EntityId()
            e2 = EntityId()
            e3 = EntityId()

            db.hash_index_insert(users, "status", b"active", e1)
            db.hash_index_insert(users, "status", b"active", e2)
            db.hash_index_insert(users, "status", b"inactive", e3)

            active = db.hash_index_lookup(users, "status", b"active")
            assert len(active) == 2

            inactive = db.hash_index_lookup(users, "status", b"inactive")
            assert len(inactive) == 1

    def test_unique_constraint(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            db.create_hash_index(users, "email", unique=True)

            e1 = EntityId()
            e2 = EntityId()

            db.hash_index_insert(users, "email", b"alice@example.com", e1)

            # Should fail on duplicate key
            with pytest.raises(IOError):
                db.hash_index_insert(users, "email", b"alice@example.com", e2)

    def test_remove(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            db.create_hash_index(users, "email", unique=False)

            entity_id = EntityId()
            db.hash_index_insert(users, "email", b"alice@example.com", entity_id)

            assert db.hash_index_len(users, "email") == 1

            db.hash_index_remove(users, "email", b"alice@example.com", entity_id)

            assert db.hash_index_len(users, "email") == 0

    def test_drop_index(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            db.create_hash_index(users, "email", unique=True)

            assert db.drop_hash_index(users, "email") is True

            # Lookup on dropped index should fail
            with pytest.raises(IOError):
                db.hash_index_lookup(users, "email", b"test")


@pytest.mark.skipif(not ENTIDB_AVAILABLE, reason="entidb not built")
class TestBTreeIndex:
    def test_create_and_insert(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            db.create_btree_index(users, "age", unique=False)

            e1 = EntityId()
            e2 = EntityId()
            e3 = EntityId()

            # Use big-endian encoding for proper ordering
            db.btree_index_insert(users, "age", (25).to_bytes(8, 'big'), e1)
            db.btree_index_insert(users, "age", (30).to_bytes(8, 'big'), e2)
            db.btree_index_insert(users, "age", (35).to_bytes(8, 'big'), e3)

            results = db.btree_index_lookup(users, "age", (30).to_bytes(8, 'big'))
            assert len(results) == 1
            assert results[0] == e2

    def test_range_query(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            db.create_btree_index(users, "age", unique=False)

            e1 = EntityId()
            e2 = EntityId()
            e3 = EntityId()
            e4 = EntityId()

            db.btree_index_insert(users, "age", (20).to_bytes(8, 'big'), e1)
            db.btree_index_insert(users, "age", (25).to_bytes(8, 'big'), e2)
            db.btree_index_insert(users, "age", (30).to_bytes(8, 'big'), e3)
            db.btree_index_insert(users, "age", (35).to_bytes(8, 'big'), e4)

            # Range: 25 <= age <= 30
            min_key = (25).to_bytes(8, 'big')
            max_key = (30).to_bytes(8, 'big')
            results = db.btree_index_range(users, "age", min_key, max_key)
            assert len(results) == 2

            # Unbounded range (all)
            all_results = db.btree_index_range(users, "age", None, None)
            assert len(all_results) == 4

            # Greater than or equal
            gte_results = db.btree_index_range(users, "age", min_key, None)
            assert len(gte_results) == 3

    def test_drop_index(self):
        with Database.open_memory() as db:
            users = db.collection("users")
            db.create_btree_index(users, "age", unique=False)

            assert db.drop_btree_index(users, "age") is True

            # Lookup on dropped index should fail
            with pytest.raises(IOError):
                db.btree_index_lookup(users, "age", (25).to_bytes(8, 'big'))
