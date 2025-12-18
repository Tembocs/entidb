/**
 * EntiDB Worker Client
 * 
 * Main thread code that communicates with the EntiDB worker.
 */

import { MessageTypes, createRequest, generateRequestId } from './shared.js';

class EntiDBClient {
    constructor() {
        this.worker = null;
        this.pending = new Map();
        this.ready = false;
        this.readyPromise = null;
    }
    
    /**
     * Initialize the worker
     */
    async init() {
        if (this.worker) return;
        
        this.worker = new Worker(new URL('./worker.js', import.meta.url), {
            type: 'module'
        });
        
        this.readyPromise = new Promise((resolve, reject) => {
            const timeout = setTimeout(() => {
                reject(new Error('Worker initialization timeout'));
            }, 10000);
            
            const onReady = (event) => {
                if (event.data.type === MessageTypes.READY) {
                    clearTimeout(timeout);
                    this.ready = true;
                    resolve();
                } else if (event.data.type === MessageTypes.ERROR) {
                    clearTimeout(timeout);
                    reject(new Error(event.data.error));
                }
            };
            
            this.worker.addEventListener('message', onReady, { once: true });
        });
        
        this.worker.onmessage = (event) => {
            this._handleMessage(event.data);
        };
        
        this.worker.onerror = (error) => {
            console.error('Worker error:', error);
        };
        
        await this.readyPromise;
    }
    
    /**
     * Send a request to the worker and wait for response
     */
    async _request(type, payload = {}) {
        if (!this.ready) {
            await this.readyPromise;
        }
        
        const id = generateRequestId();
        
        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject });
            this.worker.postMessage(createRequest(type, payload, id));
        });
    }
    
    /**
     * Handle messages from the worker
     */
    _handleMessage(message) {
        const { id, success, result, error } = message;
        
        if (!id || !this.pending.has(id)) {
            return;
        }
        
        const { resolve, reject } = this.pending.get(id);
        this.pending.delete(id);
        
        if (success) {
            resolve(result);
        } else {
            reject(new Error(error));
        }
    }
    
    // Database operations
    
    async open() {
        return this._request(MessageTypes.OPEN);
    }
    
    async close() {
        return this._request(MessageTypes.CLOSE);
    }
    
    async collection(name) {
        return this._request(MessageTypes.COLLECTION, { name });
    }
    
    async put(collection, entityId, data) {
        return this._request(MessageTypes.PUT, {
            collection,
            entityId,
            data: Array.from(data)
        });
    }
    
    async get(collection, entityId) {
        const result = await this._request(MessageTypes.GET, {
            collection,
            entityId
        });
        return result ? new Uint8Array(result) : null;
    }
    
    async delete(collection, entityId) {
        return this._request(MessageTypes.DELETE, {
            collection,
            entityId
        });
    }
    
    async list(collection) {
        const result = await this._request(MessageTypes.LIST, { collection });
        return result.map(item => ({
            id: item.id,
            data: new Uint8Array(item.data)
        }));
    }
    
    async batchPut(collection, items) {
        return this._request(MessageTypes.BATCH_PUT, {
            collection,
            items: items.map(({ entityId, data }) => ({
                entityId,
                data: Array.from(data)
            }))
        });
    }
    
    async batchDelete(collection, entityIds) {
        return this._request(MessageTypes.BATCH_DELETE, {
            collection,
            entityIds
        });
    }
    
    async generateId() {
        return this._request(MessageTypes.GENERATE_ID);
    }
    
    /**
     * Terminate the worker
     */
    terminate() {
        if (this.worker) {
            this.worker.terminate();
            this.worker = null;
            this.ready = false;
            this.pending.clear();
        }
    }
}

// Export singleton client
export const client = new EntiDBClient();
