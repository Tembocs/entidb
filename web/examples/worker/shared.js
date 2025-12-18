/**
 * Shared message types for worker communication
 */

export const MessageTypes = {
    // Lifecycle
    INIT: 'init',
    READY: 'ready',
    
    // Database operations
    OPEN: 'open',
    CLOSE: 'close',
    
    // CRUD operations
    COLLECTION: 'collection',
    PUT: 'put',
    GET: 'get',
    DELETE: 'delete',
    LIST: 'list',
    
    // Batch operations
    BATCH_PUT: 'batchPut',
    BATCH_DELETE: 'batchDelete',
    
    // Utility
    GENERATE_ID: 'generateId',
    
    // Errors
    ERROR: 'error'
};

/**
 * Create a request message
 * @param {string} type - Message type
 * @param {any} payload - Message payload
 * @param {string} id - Request ID for correlation
 */
export function createRequest(type, payload, id) {
    return { type, payload, id };
}

/**
 * Create a success response
 * @param {string} type - Message type
 * @param {any} result - Operation result
 * @param {string} id - Request ID for correlation
 */
export function createResponse(type, result, id) {
    return { type, id, success: true, result };
}

/**
 * Create an error response
 * @param {string} type - Message type
 * @param {string} error - Error message
 * @param {string} id - Request ID for correlation
 */
export function createErrorResponse(type, error, id) {
    return { type, id, success: false, error };
}

/**
 * Generate a unique request ID
 */
export function generateRequestId() {
    return `${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
}
