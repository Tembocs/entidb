/**
 * EntiDB Web Example
 * 
 * This example demonstrates using EntiDB in a web browser via WebAssembly.
 * It shows basic CRUD operations with an in-memory database.
 */

// Import the WASM module
// Note: The pkg directory is created by wasm-pack build
import init, { Database, EntityId } from './pkg/entidb_wasm.js';

// Global state
let db = null;
let usersCollection = null;

// Log helper
function log(message, type = 'info') {
    const logEl = document.getElementById('log');
    const entry = document.createElement('div');
    entry.className = `log-entry ${type}`;
    entry.textContent = `[${new Date().toLocaleTimeString()}] ${message}`;
    logEl.insertBefore(entry, logEl.firstChild);
}

// CBOR encoding helper (simplified - just wraps string in CBOR text format)
function encodeString(str) {
    // Simple text encoding for demo purposes
    // In production, use a proper CBOR library like 'cbor-x' or 'cborg'
    const encoder = new TextEncoder();
    const bytes = encoder.encode(str);
    
    // CBOR text string header (major type 3)
    // For strings < 24 bytes: single byte header
    // For strings < 256 bytes: 0x78 followed by length byte
    if (bytes.length < 24) {
        const result = new Uint8Array(1 + bytes.length);
        result[0] = 0x60 + bytes.length; // Major type 3 + length
        result.set(bytes, 1);
        return result;
    } else if (bytes.length < 256) {
        const result = new Uint8Array(2 + bytes.length);
        result[0] = 0x78; // Major type 3 + additional info 24
        result[1] = bytes.length;
        result.set(bytes, 2);
        return result;
    } else {
        // For longer strings, use 2-byte length
        const result = new Uint8Array(3 + bytes.length);
        result[0] = 0x79; // Major type 3 + additional info 25
        result[1] = (bytes.length >> 8) & 0xFF;
        result[2] = bytes.length & 0xFF;
        result.set(bytes, 3);
        return result;
    }
}

// CBOR decoding helper
function decodeString(bytes) {
    if (bytes.length === 0) return '';
    
    const header = bytes[0];
    const majorType = header >> 5;
    const additionalInfo = header & 0x1F;
    
    // Check if it's a text string (major type 3)
    if (majorType !== 3) {
        // Return hex for non-string data
        return Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join(' ');
    }
    
    let offset = 1;
    let length = additionalInfo;
    
    if (additionalInfo === 24) {
        length = bytes[1];
        offset = 2;
    } else if (additionalInfo === 25) {
        length = (bytes[1] << 8) | bytes[2];
        offset = 3;
    }
    
    const decoder = new TextDecoder();
    return decoder.decode(bytes.slice(offset, offset + length));
}

// Initialize the database
async function initDatabase() {
    try {
        // Initialize WASM module
        await init();
        log('WASM module initialized', 'success');
        
        // Open an in-memory database
        db = Database.openMemory();
        log(`Database opened (version ${db.version})`, 'success');
        
        document.getElementById('dbVersion').textContent = db.version;
        
        // Get or create the users collection
        usersCollection = db.collection('users');
        log('Created/opened "users" collection', 'success');
        
        // Show the app
        document.getElementById('loading').style.display = 'none';
        document.getElementById('app').style.display = 'block';
        
        // Initial refresh
        refreshEntityList();
        
    } catch (err) {
        log(`Failed to initialize: ${err}`, 'error');
        console.error(err);
    }
}

// Add a new entity
async function addEntity() {
    const nameInput = document.getElementById('entityName');
    const name = nameInput.value.trim();
    
    if (!name) {
        log('Please enter a name', 'error');
        return;
    }
    
    try {
        // Generate a unique ID
        const id = EntityId.generate();
        const idHex = id.toHex().substring(0, 16) + '...';
        
        // Encode the data as CBOR
        const data = encodeString(name);
        
        // Store in the database
        db.put(usersCollection, id, data);
        
        log(`Added entity: ${name} (${idHex})`, 'success');
        nameInput.value = '';
        
        refreshEntityList();
        
    } catch (err) {
        log(`Failed to add entity: ${err}`, 'error');
        console.error(err);
    }
}

// Refresh the entity list
function refreshEntityList() {
    try {
        const list = db.list(usersCollection);
        const listEl = document.getElementById('entityList');
        listEl.innerHTML = '';
        
        for (const [id, data] of list) {
            const item = document.createElement('li');
            item.className = 'entity-item';
            
            const idHex = id.toHex();
            const name = decodeString(data);
            
            item.innerHTML = `
                <div>
                    <div class="id">${idHex.substring(0, 16)}...</div>
                    <div class="data">${escapeHtml(name)}</div>
                </div>
                <button class="danger" onclick="deleteEntity('${idHex}')">Delete</button>
            `;
            listEl.appendChild(item);
        }
        
        const count = db.count(usersCollection);
        document.getElementById('entityCount').textContent = count;
        
        log(`Loaded ${count} entities`);
        
    } catch (err) {
        log(`Failed to refresh: ${err}`, 'error');
        console.error(err);
    }
}

// Delete an entity
window.deleteEntity = function(idHex) {
    try {
        const id = EntityId.fromHex(idHex);
        db.delete(usersCollection, id);
        log(`Deleted entity: ${idHex.substring(0, 16)}...`, 'success');
        refreshEntityList();
    } catch (err) {
        log(`Failed to delete: ${err}`, 'error');
        console.error(err);
    }
};

// Clear all entities
function clearAll() {
    try {
        const list = db.list(usersCollection);
        let count = 0;
        
        for (const [id, _] of list) {
            db.delete(usersCollection, id);
            count++;
        }
        
        log(`Cleared ${count} entities`, 'success');
        refreshEntityList();
        
    } catch (err) {
        log(`Failed to clear: ${err}`, 'error');
        console.error(err);
    }
}

// HTML escape helper
function escapeHtml(str) {
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
}

// Event listeners
document.addEventListener('DOMContentLoaded', () => {
    initDatabase();
    
    document.getElementById('addBtn').addEventListener('click', addEntity);
    document.getElementById('refreshBtn').addEventListener('click', refreshEntityList);
    document.getElementById('clearBtn').addEventListener('click', clearAll);
    
    // Enter key to add
    document.getElementById('entityName').addEventListener('keypress', (e) => {
        if (e.key === 'Enter') addEntity();
    });
});
