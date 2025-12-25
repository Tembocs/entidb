"""Tests for EntiDB CryptoManager Python bindings."""

import pytest

# Note: Tests require the native library to be built with encryption feature.
# Run: maturin develop

try:
    from entidb import CryptoManager, crypto_available
    CRYPTO_AVAILABLE = True
except ImportError:
    CRYPTO_AVAILABLE = False


@pytest.mark.skipif(not CRYPTO_AVAILABLE, reason="entidb crypto not built")
class TestCryptoManager:
    """Test cases for CryptoManager encryption API."""

    def test_is_available(self):
        """Test that encryption is available."""
        assert CryptoManager.is_available()
        assert crypto_available()

    def test_create_generates_unique_key(self):
        """Test that create() generates unique keys."""
        crypto1 = CryptoManager.create()
        crypto2 = CryptoManager.create()
        try:
            key1 = crypto1.get_key()
            key2 = crypto2.get_key()
            assert len(key1) == 32
            assert len(key2) == 32
            assert key1 != key2
        finally:
            crypto1.close()
            crypto2.close()

    def test_from_key_restores_encryption_context(self):
        """Test that from_key can decrypt data encrypted with the same key."""
        crypto1 = CryptoManager.create()
        key = bytes(crypto1.get_key())
        plaintext = b"test message"
        encrypted = crypto1.encrypt(plaintext)
        crypto1.close()

        # Create new crypto with same key
        crypto2 = CryptoManager.from_key(key)
        try:
            decrypted = crypto2.decrypt(encrypted)
            assert decrypted == plaintext
        finally:
            crypto2.close()

    def test_encrypt_decrypt_roundtrip(self):
        """Test basic encrypt/decrypt roundtrip."""
        crypto = CryptoManager.create()
        try:
            plaintext = b"Hello, EntiDB!"
            encrypted = crypto.encrypt(plaintext)

            # Encrypted data should be larger (nonce + ciphertext + tag)
            assert len(encrypted) == len(plaintext) + 28

            # Encrypted data should be different from plaintext
            assert encrypted != plaintext

            decrypted = crypto.decrypt(encrypted)
            assert decrypted == plaintext
        finally:
            crypto.close()

    def test_encrypt_produces_different_ciphertext(self):
        """Test that encrypting the same data produces different ciphertext."""
        crypto = CryptoManager.create()
        try:
            plaintext = b"same message"
            encrypted1 = crypto.encrypt(plaintext)
            encrypted2 = crypto.encrypt(plaintext)

            # Different nonces should produce different ciphertext
            assert encrypted1 != encrypted2

            # But both should decrypt to same plaintext
            assert crypto.decrypt(encrypted1) == plaintext
            assert crypto.decrypt(encrypted2) == plaintext
        finally:
            crypto.close()

    def test_encrypt_decrypt_with_aad_roundtrip(self):
        """Test encrypt/decrypt with AAD."""
        crypto = CryptoManager.create()
        try:
            plaintext = b"secret data"
            aad = b"entity-id-123"

            encrypted = crypto.encrypt_with_aad(plaintext, aad)
            decrypted = crypto.decrypt_with_aad(encrypted, aad)

            assert decrypted == plaintext
        finally:
            crypto.close()

    def test_decrypt_with_wrong_aad_fails(self):
        """Test that decryption fails with wrong AAD."""
        crypto = CryptoManager.create()
        try:
            plaintext = b"secret data"
            correct_aad = b"correct-aad"
            wrong_aad = b"wrong-aad"

            encrypted = crypto.encrypt_with_aad(plaintext, correct_aad)

            with pytest.raises(RuntimeError):
                crypto.decrypt_with_aad(encrypted, wrong_aad)
        finally:
            crypto.close()

    def test_decrypt_with_wrong_key_fails(self):
        """Test that decryption fails with wrong key."""
        crypto1 = CryptoManager.create()
        crypto2 = CryptoManager.create()
        try:
            plaintext = b"secret"
            encrypted = crypto1.encrypt(plaintext)

            with pytest.raises(RuntimeError):
                crypto2.decrypt(encrypted)
        finally:
            crypto1.close()
            crypto2.close()

    def test_decrypt_with_corrupted_data_fails(self):
        """Test that decryption fails with corrupted data."""
        crypto = CryptoManager.create()
        try:
            plaintext = b"original"
            encrypted = crypto.encrypt(plaintext)

            # Corrupt the ciphertext
            corrupted = bytearray(encrypted)
            corrupted[20] ^= 0xFF
            corrupted = bytes(corrupted)

            with pytest.raises(RuntimeError):
                crypto.decrypt(corrupted)
        finally:
            crypto.close()

    def test_decrypt_with_truncated_data_fails(self):
        """Test that decryption fails with truncated data."""
        crypto = CryptoManager.create()
        try:
            plaintext = b"test data"
            encrypted = crypto.encrypt(plaintext)

            # Truncate the data (too short for nonce + tag)
            truncated = encrypted[:10]

            with pytest.raises(RuntimeError):
                crypto.decrypt(truncated)
        finally:
            crypto.close()

    def test_from_password_consistent_key(self):
        """Test that same password/salt produces same key."""
        password = b"my-secret-password"
        salt = b"unique-salt-12345678"

        crypto1 = CryptoManager.from_password(password, salt)
        plaintext = b"message"
        encrypted = crypto1.encrypt(plaintext)
        crypto1.close()

        # Same password and salt should be able to decrypt
        crypto2 = CryptoManager.from_password(password, salt)
        try:
            decrypted = crypto2.decrypt(encrypted)
            assert decrypted == plaintext
        finally:
            crypto2.close()

    def test_from_password_wrong_password_fails(self):
        """Test that wrong password fails to decrypt."""
        salt = b"salt-value-123456"
        plaintext = b"secret"

        crypto1 = CryptoManager.from_password(b"correct-password", salt)
        encrypted = crypto1.encrypt(plaintext)
        crypto1.close()

        crypto2 = CryptoManager.from_password(b"wrong-password", salt)
        try:
            with pytest.raises(RuntimeError):
                crypto2.decrypt(encrypted)
        finally:
            crypto2.close()

    def test_from_password_different_salt_fails(self):
        """Test that different salt fails to decrypt."""
        password = b"same-password"
        salt1 = b"salt-one-123456"
        salt2 = b"salt-two-654321"
        plaintext = b"secret"

        crypto1 = CryptoManager.from_password(password, salt1)
        encrypted = crypto1.encrypt(plaintext)
        crypto1.close()

        crypto2 = CryptoManager.from_password(password, salt2)
        try:
            with pytest.raises(RuntimeError):
                crypto2.decrypt(encrypted)
        finally:
            crypto2.close()

    def test_from_key_wrong_length(self):
        """Test that from_key rejects wrong key length."""
        with pytest.raises(ValueError):
            CryptoManager.from_key(bytes(16))
        with pytest.raises(ValueError):
            CryptoManager.from_key(bytes(64))

    def test_operations_after_close_fail(self):
        """Test that operations after close raise an error."""
        crypto = CryptoManager.create()
        crypto.close()

        with pytest.raises(RuntimeError):
            crypto.encrypt(b"test")
        with pytest.raises(RuntimeError):
            crypto.decrypt(bytes(50))

    def test_close_is_idempotent(self):
        """Test that close can be called multiple times."""
        crypto = CryptoManager.create()
        assert not crypto.is_closed

        crypto.close()
        assert crypto.is_closed

        # Second close should not raise
        crypto.close()
        assert crypto.is_closed

    def test_context_manager(self):
        """Test that CryptoManager works as a context manager."""
        plaintext = b"test data"

        with CryptoManager.create() as crypto:
            assert not crypto.is_closed
            encrypted = crypto.encrypt(plaintext)
            assert len(encrypted) > 0

        assert crypto.is_closed

    def test_encrypt_empty_data(self):
        """Test encrypting empty data."""
        crypto = CryptoManager.create()
        try:
            empty = b""
            encrypted = crypto.encrypt(empty)

            # Should have overhead but no plaintext bytes
            assert len(encrypted) == 28  # 12 (nonce) + 0 + 16 (tag)

            decrypted = crypto.decrypt(encrypted)
            assert decrypted == b""
        finally:
            crypto.close()

    def test_encrypt_large_data(self):
        """Test encrypting large data."""
        crypto = CryptoManager.create()
        try:
            # 1 MB of data
            large = bytes([i % 256 for i in range(1024 * 1024)])
            encrypted = crypto.encrypt(large)

            assert len(encrypted) == len(large) + 28

            decrypted = crypto.decrypt(encrypted)
            assert decrypted == large
        finally:
            crypto.close()

    def test_encrypt_with_empty_aad(self):
        """Test encryption with empty AAD."""
        crypto = CryptoManager.create()
        try:
            plaintext = b"data"
            empty_aad = b""

            encrypted = crypto.encrypt_with_aad(plaintext, empty_aad)
            decrypted = crypto.decrypt_with_aad(encrypted, empty_aad)

            assert decrypted == plaintext
        finally:
            crypto.close()

    def test_encrypt_with_large_aad(self):
        """Test encryption with large AAD."""
        crypto = CryptoManager.create()
        try:
            plaintext = b"data"
            large_aad = bytes([i % 256 for i in range(10000)])

            encrypted = crypto.encrypt_with_aad(plaintext, large_aad)
            decrypted = crypto.decrypt_with_aad(encrypted, large_aad)

            assert decrypted == plaintext
        finally:
            crypto.close()

    def test_repr(self):
        """Test CryptoManager repr."""
        crypto = CryptoManager.create()
        assert "active" in repr(crypto)

        crypto.close()
        assert "closed" in repr(crypto)
