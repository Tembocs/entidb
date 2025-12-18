/**
 * EntiDB Store Wrapper
 * 
 * Provides a typed store interface for todo items using EntiDB.
 */

import init, { Database, EntityId } from './pkg/entidb_wasm.js';
import { encode, decode } from './cbor.js';

/**
 * Todo item structure
 * @typedef {Object} Todo
 * @property {string} id - Entity ID as hex string
 * @property {string} title - Todo title
 * @property {boolean} completed - Whether the todo is completed
 * @property {number} createdAt - Creation timestamp (ms since epoch)
 */

let db = null;
let todosCollection = null;
let initialized = false;

/**
 * Initialize the EntiDB store
 * @returns {Promise<void>}
 */
export async function initStore() {
    if (initialized) return;
    
    await init();
    db = Database.openMemory();
    todosCollection = db.collection('todos');
    initialized = true;
}

/**
 * Create a new todo
 * @param {string} title - Todo title
 * @returns {Todo} The created todo
 */
export function createTodo(title) {
    ensureInitialized();
    
    const id = EntityId.generate();
    const todo = {
        title,
        completed: false,
        createdAt: Date.now()
    };
    
    const bytes = encode(todo);
    db.put(todosCollection, id, bytes);
    
    return {
        id: id.toHex(),
        ...todo
    };
}

/**
 * Get all todos
 * @returns {Todo[]} All todos
 */
export function getAllTodos() {
    ensureInitialized();
    
    const entities = db.list(todosCollection);
    const todos = [];
    
    for (const [id, bytes] of entities) {
        const data = decode(bytes);
        todos.push({
            id: id.toHex(),
            ...data
        });
    }
    
    // Sort by creation time (newest first)
    todos.sort((a, b) => b.createdAt - a.createdAt);
    
    return todos;
}

/**
 * Get a todo by ID
 * @param {string} idHex - Entity ID as hex string
 * @returns {Todo|null} The todo or null if not found
 */
export function getTodo(idHex) {
    ensureInitialized();
    
    const id = EntityId.fromHex(idHex);
    const bytes = db.get(todosCollection, id);
    
    if (!bytes) return null;
    
    const data = decode(bytes);
    return {
        id: idHex,
        ...data
    };
}

/**
 * Update a todo
 * @param {string} idHex - Entity ID as hex string
 * @param {Partial<Todo>} updates - Fields to update
 * @returns {Todo|null} The updated todo or null if not found
 */
export function updateTodo(idHex, updates) {
    ensureInitialized();
    
    const existing = getTodo(idHex);
    if (!existing) return null;
    
    const updated = {
        title: updates.title ?? existing.title,
        completed: updates.completed ?? existing.completed,
        createdAt: existing.createdAt
    };
    
    const id = EntityId.fromHex(idHex);
    const bytes = encode(updated);
    db.put(todosCollection, id, bytes);
    
    return {
        id: idHex,
        ...updated
    };
}

/**
 * Toggle a todo's completed status
 * @param {string} idHex - Entity ID as hex string
 * @returns {Todo|null} The updated todo or null if not found
 */
export function toggleTodo(idHex) {
    const existing = getTodo(idHex);
    if (!existing) return null;
    
    return updateTodo(idHex, { completed: !existing.completed });
}

/**
 * Delete a todo
 * @param {string} idHex - Entity ID as hex string
 * @returns {boolean} Whether the todo was deleted
 */
export function deleteTodo(idHex) {
    ensureInitialized();
    
    const id = EntityId.fromHex(idHex);
    const exists = db.get(todosCollection, id) !== null;
    
    if (exists) {
        db.delete(todosCollection, id);
    }
    
    return exists;
}

/**
 * Delete all completed todos
 * @returns {number} Number of todos deleted
 */
export function clearCompleted() {
    ensureInitialized();
    
    const todos = getAllTodos();
    let deleted = 0;
    
    for (const todo of todos) {
        if (todo.completed) {
            deleteTodo(todo.id);
            deleted++;
        }
    }
    
    return deleted;
}

/**
 * Get counts of todos by status
 * @returns {{total: number, active: number, completed: number}}
 */
export function getCounts() {
    const todos = getAllTodos();
    const completed = todos.filter(t => t.completed).length;
    
    return {
        total: todos.length,
        active: todos.length - completed,
        completed
    };
}

function ensureInitialized() {
    if (!initialized) {
        throw new Error('Store not initialized. Call initStore() first.');
    }
}
