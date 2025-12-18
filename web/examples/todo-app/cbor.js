/**
 * CBOR Encoding/Decoding Utilities
 * 
 * A minimal CBOR implementation for encoding/decoding todo items.
 * For production use, consider a full CBOR library like 'cborg' or 'cbor-x'.
 */

/**
 * Encode a value to CBOR bytes
 * @param {any} value - Value to encode
 * @returns {Uint8Array} CBOR-encoded bytes
 */
export function encode(value) {
    const parts = [];
    encodeValue(value, parts);
    
    const totalLength = parts.reduce((sum, p) => sum + p.length, 0);
    const result = new Uint8Array(totalLength);
    let offset = 0;
    for (const part of parts) {
        result.set(part, offset);
        offset += part.length;
    }
    return result;
}

/**
 * Decode CBOR bytes to a value
 * @param {Uint8Array} bytes - CBOR-encoded bytes
 * @returns {any} Decoded value
 */
export function decode(bytes) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const result = decodeValue(bytes, view, 0);
    return result.value;
}

function encodeValue(value, parts) {
    if (value === null || value === undefined) {
        parts.push(new Uint8Array([0xf6])); // CBOR null
    } else if (typeof value === 'boolean') {
        parts.push(new Uint8Array([value ? 0xf5 : 0xf4]));
    } else if (typeof value === 'number') {
        if (Number.isInteger(value)) {
            encodeInteger(value, parts);
        } else {
            encodeFloat(value, parts);
        }
    } else if (typeof value === 'string') {
        encodeString(value, parts);
    } else if (Array.isArray(value)) {
        encodeArray(value, parts);
    } else if (typeof value === 'object') {
        encodeMap(value, parts);
    } else {
        throw new Error(`Unsupported type: ${typeof value}`);
    }
}

function encodeInteger(value, parts) {
    if (value >= 0) {
        // Unsigned integer (major type 0)
        encodeTypeAndLength(0, value, parts);
    } else {
        // Negative integer (major type 1)
        encodeTypeAndLength(1, -1 - value, parts);
    }
}

function encodeTypeAndLength(majorType, length, parts) {
    const typeShift = majorType << 5;
    
    if (length < 24) {
        parts.push(new Uint8Array([typeShift | length]));
    } else if (length < 256) {
        parts.push(new Uint8Array([typeShift | 24, length]));
    } else if (length < 65536) {
        const bytes = new Uint8Array(3);
        bytes[0] = typeShift | 25;
        bytes[1] = (length >> 8) & 0xff;
        bytes[2] = length & 0xff;
        parts.push(bytes);
    } else if (length < 4294967296) {
        const bytes = new Uint8Array(5);
        bytes[0] = typeShift | 26;
        bytes[1] = (length >> 24) & 0xff;
        bytes[2] = (length >> 16) & 0xff;
        bytes[3] = (length >> 8) & 0xff;
        bytes[4] = length & 0xff;
        parts.push(bytes);
    } else {
        throw new Error('Value too large');
    }
}

function encodeFloat(value, parts) {
    // Use float64 for all floats
    const bytes = new Uint8Array(9);
    bytes[0] = 0xfb; // float64
    const view = new DataView(bytes.buffer);
    view.setFloat64(1, value, false);
    parts.push(bytes);
}

function encodeString(value, parts) {
    const encoder = new TextEncoder();
    const utf8 = encoder.encode(value);
    encodeTypeAndLength(3, utf8.length, parts);
    parts.push(utf8);
}

function encodeArray(value, parts) {
    encodeTypeAndLength(4, value.length, parts);
    for (const item of value) {
        encodeValue(item, parts);
    }
}

function encodeMap(value, parts) {
    // Sort keys for canonical CBOR
    const keys = Object.keys(value).sort((a, b) => {
        // Sort by length first, then lexicographically
        if (a.length !== b.length) return a.length - b.length;
        return a < b ? -1 : a > b ? 1 : 0;
    });
    
    encodeTypeAndLength(5, keys.length, parts);
    for (const key of keys) {
        encodeString(key, parts);
        encodeValue(value[key], parts);
    }
}

function decodeValue(bytes, view, offset) {
    const header = bytes[offset];
    const majorType = header >> 5;
    const additionalInfo = header & 0x1f;
    
    switch (majorType) {
        case 0: // Unsigned integer
            return decodeUnsigned(bytes, view, offset);
        case 1: // Negative integer
            const unsigned = decodeUnsigned(bytes, view, offset);
            return { value: -1 - unsigned.value, offset: unsigned.offset };
        case 2: // Byte string
            return decodeBytes(bytes, view, offset);
        case 3: // Text string
            return decodeText(bytes, view, offset);
        case 4: // Array
            return decodeArray(bytes, view, offset);
        case 5: // Map
            return decodeMap(bytes, view, offset);
        case 7: // Simple/float
            return decodeSimple(bytes, view, offset);
        default:
            throw new Error(`Unsupported major type: ${majorType}`);
    }
}

function decodeLength(bytes, view, offset) {
    const header = bytes[offset];
    const additionalInfo = header & 0x1f;
    
    if (additionalInfo < 24) {
        return { value: additionalInfo, offset: offset + 1 };
    } else if (additionalInfo === 24) {
        return { value: bytes[offset + 1], offset: offset + 2 };
    } else if (additionalInfo === 25) {
        return { value: view.getUint16(offset + 1, false), offset: offset + 3 };
    } else if (additionalInfo === 26) {
        return { value: view.getUint32(offset + 1, false), offset: offset + 5 };
    } else if (additionalInfo === 27) {
        // 64-bit - use BigInt but convert to Number
        const high = view.getUint32(offset + 1, false);
        const low = view.getUint32(offset + 5, false);
        return { value: high * 0x100000000 + low, offset: offset + 9 };
    }
    
    throw new Error(`Unsupported additional info: ${additionalInfo}`);
}

function decodeUnsigned(bytes, view, offset) {
    return decodeLength(bytes, view, offset);
}

function decodeBytes(bytes, view, offset) {
    const length = decodeLength(bytes, view, offset);
    const data = bytes.slice(length.offset, length.offset + length.value);
    return { value: data, offset: length.offset + length.value };
}

function decodeText(bytes, view, offset) {
    const length = decodeLength(bytes, view, offset);
    const data = bytes.slice(length.offset, length.offset + length.value);
    const decoder = new TextDecoder();
    return { value: decoder.decode(data), offset: length.offset + length.value };
}

function decodeArray(bytes, view, offset) {
    const length = decodeLength(bytes, view, offset);
    const result = [];
    let currentOffset = length.offset;
    
    for (let i = 0; i < length.value; i++) {
        const item = decodeValue(bytes, view, currentOffset);
        result.push(item.value);
        currentOffset = item.offset;
    }
    
    return { value: result, offset: currentOffset };
}

function decodeMap(bytes, view, offset) {
    const length = decodeLength(bytes, view, offset);
    const result = {};
    let currentOffset = length.offset;
    
    for (let i = 0; i < length.value; i++) {
        const key = decodeValue(bytes, view, currentOffset);
        currentOffset = key.offset;
        const value = decodeValue(bytes, view, currentOffset);
        currentOffset = value.offset;
        result[key.value] = value.value;
    }
    
    return { value: result, offset: currentOffset };
}

function decodeSimple(bytes, view, offset) {
    const header = bytes[offset];
    const additionalInfo = header & 0x1f;
    
    switch (additionalInfo) {
        case 20: return { value: false, offset: offset + 1 };
        case 21: return { value: true, offset: offset + 1 };
        case 22: return { value: null, offset: offset + 1 };
        case 23: return { value: undefined, offset: offset + 1 };
        case 25: // float16
            throw new Error('float16 not supported');
        case 26: // float32
            return { value: view.getFloat32(offset + 1, false), offset: offset + 5 };
        case 27: // float64
            return { value: view.getFloat64(offset + 1, false), offset: offset + 9 };
        default:
            throw new Error(`Unsupported simple value: ${additionalInfo}`);
    }
}
