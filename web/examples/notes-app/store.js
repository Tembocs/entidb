/**
 * Notes Store - EntiDB Persistent Storage Wrapper
 * 
 * This module wraps EntiDB for storing notes with persistence.
 */

import init, { Database } from './pkg/entidb_wasm.js';

let db = null;
let notesCollection = null;
let initialized = false;
let storageInfo = {
    type: 'unknown',
    persistent: false,
    available: false
};

/**
 * Encodes a note object to CBOR bytes.
 */
function encodeNote(note) {
    // Encode as CBOR map with 4 entries
    // Map header: 0xa4 (map with 4 items)
    const parts = [];
    
    // Add map header (4 items)
    parts.push(0xa4);
    
    // Key: "id" (2 chars)
    parts.push(0x62, 0x69, 0x64); // "id"
    // Value: id string
    const idBytes = new TextEncoder().encode(note.id);
    if (idBytes.length < 24) {
        parts.push(0x60 + idBytes.length);
    } else {
        parts.push(0x78, idBytes.length);
    }
    parts.push(...idBytes);
    
    // Key: "title" (5 chars)
    parts.push(0x65, 0x74, 0x69, 0x74, 0x6c, 0x65); // "title"
    // Value: title string
    const titleBytes = new TextEncoder().encode(note.title);
    if (titleBytes.length < 24) {
        parts.push(0x60 + titleBytes.length);
    } else if (titleBytes.length < 256) {
        parts.push(0x78, titleBytes.length);
    } else {
        parts.push(0x79, (titleBytes.length >> 8) & 0xFF, titleBytes.length & 0xFF);
    }
    parts.push(...titleBytes);
    
    // Key: "content" (7 chars)
    parts.push(0x67, 0x63, 0x6f, 0x6e, 0x74, 0x65, 0x6e, 0x74); // "content"
    // Value: content string
    const contentBytes = new TextEncoder().encode(note.content);
    if (contentBytes.length < 24) {
        parts.push(0x60 + contentBytes.length);
    } else if (contentBytes.length < 256) {
        parts.push(0x78, contentBytes.length);
    } else {
        parts.push(0x79, (contentBytes.length >> 8) & 0xFF, contentBytes.length & 0xFF);
    }
    parts.push(...contentBytes);
    
    // Key: "updated" (7 chars)
    parts.push(0x67, 0x75, 0x70, 0x64, 0x61, 0x74, 0x65, 0x64); // "updated"
    // Value: timestamp as integer
    const ts = note.updated;
    if (ts < 24) {
        parts.push(ts);
    } else if (ts < 256) {
        parts.push(0x18, ts);
    } else if (ts < 65536) {
        parts.push(0x19, (ts >> 8) & 0xFF, ts & 0xFF);
    } else if (ts < 4294967296) {
        parts.push(0x1a, (ts >> 24) & 0xFF, (ts >> 16) & 0xFF, (ts >> 8) & 0xFF, ts & 0xFF);
    } else {
        // 64-bit integer
        parts.push(0x1b);
        const high = Math.floor(ts / 0x100000000);
        const low = ts >>> 0;
        parts.push(
            (high >> 24) & 0xFF, (high >> 16) & 0xFF, (high >> 8) & 0xFF, high & 0xFF,
            (low >> 24) & 0xFF, (low >> 16) & 0xFF, (low >> 8) & 0xFF, low & 0xFF
        );
    }
    
    return new Uint8Array(parts);
}

/**
 * Decodes CBOR bytes to a note object.
 */
function decodeNote(bytes) {
    const decoder = new TextDecoder();
    let pos = 0;
    
    function readByte() {
        return bytes[pos++];
    }
    
    function readString() {
        const header = readByte();
        const majorType = header >> 5;
        let length = header & 0x1F;
        
        if (majorType !== 3) throw new Error('Expected string');
        
        if (length === 24) length = readByte();
        else if (length === 25) length = (readByte() << 8) | readByte();
        
        const strBytes = bytes.slice(pos, pos + length);
        pos += length;
        return decoder.decode(strBytes);
    }
    
    function readInt() {
        const header = readByte();
        const majorType = header >> 5;
        let value = header & 0x1F;
        
        if (majorType !== 0) throw new Error('Expected integer');
        
        if (value === 24) value = readByte();
        else if (value === 25) value = (readByte() << 8) | readByte();
        else if (value === 26) value = (readByte() << 24) | (readByte() << 16) | (readByte() << 8) | readByte();
        else if (value === 27) {
            const high = (readByte() << 24) | (readByte() << 16) | (readByte() << 8) | readByte();
            const low = (readByte() << 24) | (readByte() << 16) | (readByte() << 8) | readByte();
            value = high * 0x100000000 + (low >>> 0);
        }
        
        return value;
    }
    
    // Read map header
    const header = readByte();
    if ((header >> 5) !== 5) throw new Error('Expected map');
    const mapSize = header & 0x1F;
    
    const note = {};
    for (let i = 0; i < mapSize; i++) {
        const key = readString();
        if (key === 'updated') {
            note[key] = readInt();
        } else {
            note[key] = readString();
        }
    }
    
    return note;
}

/**
 * Initialize the store with persistent storage.
 */
export async function initStore() {
    if (initialized) return storageInfo;
    
    try {
        // Initialize WASM
        await init();
        
        // Try to open a persistent database
        try {
            db = await Database.open('notes-app');
            storageInfo = {
                type: db.storageType,
                persistent: db.isPersistent,
                available: true
            };
        } catch (e) {
            // Fall back to in-memory
            console.warn('Persistent storage unavailable, using in-memory:', e);
            db = Database.openMemory();
            storageInfo = {
                type: 'memory',
                persistent: false,
                available: true
            };
        }
        
        // Get the notes collection
        notesCollection = db.collection('notes');
        initialized = true;
        
        return storageInfo;
    } catch (error) {
        console.error('Failed to initialize store:', error);
        throw error;
    }
}

/**
 * Get storage information.
 */
export function getStorageInfo() {
    return storageInfo;
}

/**
 * Save a note.
 */
export async function saveNote(note) {
    if (!initialized) throw new Error('Store not initialized');
    
    const noteData = {
        ...note,
        updated: Date.now()
    };
    
    const bytes = encodeNote(noteData);
    db.put(notesCollection, note.id, bytes);
    
    // Persist if using persistent storage
    if (db.isPersistent && db.hasUnsavedChanges) {
        await db.save();
    }
    
    return noteData;
}

/**
 * Get a note by ID.
 */
export function getNote(id) {
    if (!initialized) throw new Error('Store not initialized');
    
    const bytes = db.get(notesCollection, id);
    if (!bytes) return null;
    
    return decodeNote(bytes);
}

/**
 * Delete a note.
 */
export async function deleteNote(id) {
    if (!initialized) throw new Error('Store not initialized');
    
    db.delete(notesCollection, id);
    
    // Persist if using persistent storage
    if (db.isPersistent && db.hasUnsavedChanges) {
        await db.save();
    }
}

/**
 * Get all notes.
 */
export function getAllNotes() {
    if (!initialized) throw new Error('Store not initialized');
    
    const notes = [];
    const iterator = db.iter(notesCollection);
    
    while (iterator.hasNext()) {
        const { entityId, data } = iterator.next();
        if (data) {
            try {
                const note = decodeNote(data);
                notes.push(note);
            } catch (e) {
                console.warn('Failed to decode note:', entityId, e);
            }
        }
    }
    
    iterator.free();
    
    // Sort by updated timestamp, newest first
    notes.sort((a, b) => b.updated - a.updated);
    return notes;
}

/**
 * Search notes by query.
 */
export function searchNotes(query) {
    const allNotes = getAllNotes();
    if (!query) return allNotes;
    
    const q = query.toLowerCase();
    return allNotes.filter(note => 
        note.title.toLowerCase().includes(q) ||
        note.content.toLowerCase().includes(q)
    );
}

/**
 * Generate a unique ID.
 */
export function generateId() {
    return crypto.randomUUID();
}

/**
 * Check if there are unsaved changes.
 */
export function hasUnsavedChanges() {
    if (!db) return false;
    return db.hasUnsavedChanges;
}

/**
 * Force save to persistent storage.
 */
export async function forceSave() {
    if (!db || !db.isPersistent) return;
    await db.save();
}
