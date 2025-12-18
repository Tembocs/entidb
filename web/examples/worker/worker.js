/**
 * EntiDB Web Worker
 * 
 * Runs EntiDB operations in a dedicated worker thread.
 */

import init, { Database, EntityId } from './pkg/entidb_wasm.js';
import { MessageTypes, createResponse, createErrorResponse } from './shared.js';

let db = null;
const collections = new Map();

// Initialize WASM on worker start
async function initialize() {
    try {
        await init();
        self.postMessage({ type: MessageTypes.READY });
    } catch (error) {
        self.postMessage({
            type: MessageTypes.ERROR,
            error: `Failed to initialize WASM: ${error.message}`
        });
    }
}

// Handle messages from main thread
self.onmessage = async function(event) {
    const { type, payload, id } = event.data;
    
    try {
        let result;
        
        switch (type) {
            case MessageTypes.OPEN:
                result = handleOpen(payload);
                break;
                
            case MessageTypes.CLOSE:
                result = handleClose();
                break;
                
            case MessageTypes.COLLECTION:
                result = handleCollection(payload);
                break;
                
            case MessageTypes.PUT:
                result = handlePut(payload);
                break;
                
            case MessageTypes.GET:
                result = handleGet(payload);
                break;
                
            case MessageTypes.DELETE:
                result = handleDelete(payload);
                break;
                
            case MessageTypes.LIST:
                result = handleList(payload);
                break;
                
            case MessageTypes.BATCH_PUT:
                result = handleBatchPut(payload);
                break;
                
            case MessageTypes.BATCH_DELETE:
                result = handleBatchDelete(payload);
                break;
                
            case MessageTypes.GENERATE_ID:
                result = handleGenerateId();
                break;
                
            default:
                throw new Error(`Unknown message type: ${type}`);
        }
        
        self.postMessage(createResponse(type, result, id));
        
    } catch (error) {
        self.postMessage(createErrorResponse(type, error.message, id));
    }
};

function handleOpen(payload) {
    if (db) {
        db.close();
    }
    
    // For now, only memory database is supported
    db = Database.openMemory();
    collections.clear();
    
    return { version: db.version };
}

function handleClose() {
    if (db) {
        db.close();
        db = null;
        collections.clear();
    }
    return true;
}

function handleCollection(payload) {
    ensureDatabase();
    const { name } = payload;
    
    if (!collections.has(name)) {
        const collection = db.collection(name);
        collections.set(name, collection);
    }
    
    return { name };
}

function handlePut(payload) {
    ensureDatabase();
    const { collection: collectionName, entityId, data } = payload;
    
    const collection = getCollection(collectionName);
    const id = EntityId.fromHex(entityId);
    const bytes = new Uint8Array(data);
    
    db.put(collection, id, bytes);
    
    return true;
}

function handleGet(payload) {
    ensureDatabase();
    const { collection: collectionName, entityId } = payload;
    
    const collection = getCollection(collectionName);
    const id = EntityId.fromHex(entityId);
    
    const bytes = db.get(collection, id);
    
    if (bytes === null || bytes === undefined) {
        return null;
    }
    
    return Array.from(bytes);
}

function handleDelete(payload) {
    ensureDatabase();
    const { collection: collectionName, entityId } = payload;
    
    const collection = getCollection(collectionName);
    const id = EntityId.fromHex(entityId);
    
    db.delete(collection, id);
    
    return true;
}

function handleList(payload) {
    ensureDatabase();
    const { collection: collectionName } = payload;
    
    const collection = getCollection(collectionName);
    const entities = db.list(collection);
    
    return entities.map(([id, bytes]) => ({
        id: id.toHex(),
        data: Array.from(bytes)
    }));
}

function handleBatchPut(payload) {
    ensureDatabase();
    const { collection: collectionName, items } = payload;
    
    const collection = getCollection(collectionName);
    let count = 0;
    
    for (const { entityId, data } of items) {
        const id = EntityId.fromHex(entityId);
        const bytes = new Uint8Array(data);
        db.put(collection, id, bytes);
        count++;
    }
    
    return { count };
}

function handleBatchDelete(payload) {
    ensureDatabase();
    const { collection: collectionName, entityIds } = payload;
    
    const collection = getCollection(collectionName);
    let count = 0;
    
    for (const entityId of entityIds) {
        const id = EntityId.fromHex(entityId);
        db.delete(collection, id);
        count++;
    }
    
    return { count };
}

function handleGenerateId() {
    const id = EntityId.generate();
    return id.toHex();
}

function ensureDatabase() {
    if (!db) {
        throw new Error('Database not open. Call open() first.');
    }
}

function getCollection(name) {
    if (!collections.has(name)) {
        const collection = db.collection(name);
        collections.set(name, collection);
    }
    return collections.get(name);
}

// Start initialization
initialize();
