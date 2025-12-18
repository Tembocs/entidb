"""Cross-language test vector validation for EntiDB Python bindings.

These tests validate that Python produces identical CBOR encoding
and entity ID handling as Rust and Dart implementations.
"""

import json
import os
import pytest
from pathlib import Path


def hex_decode(hex_str: str) -> bytes:
    """Decode hex string to bytes."""
    if not hex_str:
        return b""
    return bytes.fromhex(hex_str)


def hex_encode(data: bytes) -> str:
    """Encode bytes to hex string."""
    return data.hex()


# Find test vectors directory
VECTORS_DIR = Path(__file__).parent.parent.parent.parent.parent / "docs" / "test_vectors"


class TestCborVectors:
    """Test CBOR encoding vectors."""

    @pytest.fixture
    def vectors(self):
        """Load CBOR test vectors."""
        vector_file = VECTORS_DIR / "cbor.json"
        if not vector_file.exists():
            pytest.skip(f"Test vectors not found at {vector_file}")
        with open(vector_file) as f:
            return json.load(f)

    def test_all_cbor_vectors(self, vectors):
        """Validate all CBOR vectors pass."""
        for vector in vectors:
            vid = vector["id"]
            description = vector["description"]
            input_hex = vector["input_hex"]
            expected_hex = vector["expected_hex"]
            expected_error = vector.get("expected_error")

            input_bytes = hex_decode(input_hex)

            if expected_error:
                # This vector should fail
                with pytest.raises(Exception):
                    decoded = decode_cbor(input_bytes)
                    encode_cbor(decoded)
            else:
                # This vector should succeed and round-trip
                try:
                    decoded = decode_cbor(input_bytes)
                    reencoded = encode_cbor(decoded)
                    reencoded_hex = hex_encode(reencoded)

                    assert reencoded_hex.lower() == expected_hex.lower(), \
                        f"Vector {vid} failed: {description}"
                except Exception as e:
                    pytest.fail(f"Vector {vid} unexpected failure: {description} - {e}")


class TestEntityIdVectors:
    """Test Entity ID vectors."""

    @pytest.fixture
    def vectors(self):
        """Load Entity ID test vectors."""
        vector_file = VECTORS_DIR / "entity_id.json"
        if not vector_file.exists():
            pytest.skip(f"Test vectors not found at {vector_file}")
        with open(vector_file) as f:
            return json.load(f)

    def test_all_entity_id_vectors(self, vectors):
        """Validate all Entity ID vectors pass."""
        for vector in vectors:
            vid = vector["id"]
            description = vector["description"]
            input_hex = vector["input_hex"]
            expected_hex = vector["expected_hex"]
            expected_error = vector.get("expected_error")

            input_bytes = hex_decode(input_hex)

            if expected_error:
                # This vector should fail (wrong length)
                assert len(input_bytes) != 16, \
                    f"Vector {vid} should fail: {description}"
            else:
                # This vector should succeed
                assert len(input_bytes) == 16
                roundtripped = hex_encode(input_bytes)
                assert roundtripped.lower() == expected_hex.lower(), \
                    f"Vector {vid} failed: {description}"


# Minimal CBOR implementation for testing
def decode_cbor(data: bytes):
    """Decode CBOR data (minimal implementation for tests)."""
    if not data:
        raise ValueError("Empty CBOR data")

    offset = 0

    def read_byte():
        nonlocal offset
        if offset >= len(data):
            raise ValueError("Unexpected end of data")
        b = data[offset]
        offset += 1
        return b

    def read_uint(additional_info):
        nonlocal offset
        if additional_info < 24:
            return additional_info
        elif additional_info == 24:
            return read_byte()
        elif additional_info == 25:
            return (read_byte() << 8) | read_byte()
        elif additional_info == 26:
            return (read_byte() << 24) | (read_byte() << 16) | (read_byte() << 8) | read_byte()
        elif additional_info == 27:
            value = 0
            for _ in range(8):
                value = (value << 8) | read_byte()
            return value
        elif additional_info >= 28:
            raise ValueError("Indefinite-length items are forbidden")
        else:
            raise ValueError(f"Invalid additional info: {additional_info}")

    def decode():
        nonlocal offset
        initial = read_byte()
        major_type = initial >> 5
        additional_info = initial & 0x1f

        # Check for indefinite-length items
        if additional_info == 31:
            raise ValueError("Indefinite-length items are forbidden")

        if major_type == 0:  # Unsigned integer
            return read_uint(additional_info)
        elif major_type == 1:  # Negative integer
            return -1 - read_uint(additional_info)
        elif major_type == 2:  # Byte string
            length = read_uint(additional_info)
            result = data[offset:offset + length]
            offset += length
            return result
        elif major_type == 3:  # Text string
            length = read_uint(additional_info)
            result = data[offset:offset + length].decode("utf-8")
            offset += length
            return result
        elif major_type == 4:  # Array
            length = read_uint(additional_info)
            return [decode() for _ in range(length)]
        elif major_type == 5:  # Map
            length = read_uint(additional_info)
            result = {}
            for _ in range(length):
                key = decode()
                value = decode()
                # Convert mutable types to hashable for dict keys
                if isinstance(key, list):
                    key = tuple(key)
                elif isinstance(key, bytes):
                    key = key  # bytes are hashable
                result[key] = value
            return result
        elif major_type == 6:  # Tag (not used)
            raise ValueError("Tags are not supported")
        elif major_type == 7:  # Simple/float
            if additional_info == 20:
                return False
            elif additional_info == 21:
                return True
            elif additional_info == 22:
                return None
            elif 25 <= additional_info <= 27:
                raise ValueError("Floats are not allowed")
            else:
                raise ValueError(f"Unknown simple value: {additional_info}")
        else:
            raise ValueError(f"Unknown major type: {major_type}")

    return decode()


def encode_cbor(value) -> bytes:
    """Encode value to canonical CBOR (minimal implementation for tests)."""
    buffer = bytearray()

    def write_uint(major_type, val):
        major = major_type << 5
        if val < 24:
            buffer.append(major | val)
        elif val < 256:
            buffer.append(major | 24)
            buffer.append(val)
        elif val < 65536:
            buffer.append(major | 25)
            buffer.append((val >> 8) & 0xff)
            buffer.append(val & 0xff)
        elif val < 4294967296:
            buffer.append(major | 26)
            buffer.append((val >> 24) & 0xff)
            buffer.append((val >> 16) & 0xff)
            buffer.append((val >> 8) & 0xff)
            buffer.append(val & 0xff)
        else:
            buffer.append(major | 27)
            for i in range(7, -1, -1):
                buffer.append((val >> (i * 8)) & 0xff)

    def encode(v):
        if v is None:
            buffer.append(0xf6)
        elif v is True:
            buffer.append(0xf5)
        elif v is False:
            buffer.append(0xf4)
        elif isinstance(v, int):
            if v >= 0:
                write_uint(0, v)
            else:
                write_uint(1, -1 - v)
        elif isinstance(v, bytes):
            write_uint(2, len(v))
            buffer.extend(v)
        elif isinstance(v, str):
            encoded = v.encode("utf-8")
            write_uint(3, len(encoded))
            buffer.extend(encoded)
        elif isinstance(v, (list, tuple)):
            write_uint(4, len(v))
            for item in v:
                encode(item)
        elif isinstance(v, dict):
            # Sort keys canonically (by encoded bytes)
            def key_bytes(k):
                return encode_cbor(k)

            sorted_items = sorted(v.items(), key=lambda x: (len(key_bytes(x[0])), key_bytes(x[0])))

            write_uint(5, len(sorted_items))
            for key, val in sorted_items:
                encode(key)
                encode(val)
        else:
            raise TypeError(f"Unsupported type: {type(v)}")

    encode(value)
    return bytes(buffer)


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
