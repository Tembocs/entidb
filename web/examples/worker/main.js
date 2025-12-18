/**
 * EntiDB Web Worker Example - Main Thread
 */

import { client } from './client.js';

// DOM Elements
const workerStatusEl = document.getElementById('workerStatus');
const workerStatusTextEl = document.getElementById('workerStatusText');
const entityCountEl = document.getElementById('entityCount');
const opCountEl = document.getElementById('opCount');
const logEl = document.getElementById('log');

const putCollectionEl = document.getElementById('putCollection');
const putEntityIdEl = document.getElementById('putEntityId');
const putDataEl = document.getElementById('putData');
const putBtnEl = document.getElementById('putBtn');
const putManyBtnEl = document.getElementById('putManyBtn');

const getCollectionEl = document.getElementById('getCollection');
const getEntityIdEl = document.getElementById('getEntityId');
const getBtnEl = document.getElementById('getBtn');
const listBtnEl = document.getElementById('listBtn');
const getOutputEl = document.getElementById('getOutput');

const benchmarkCountEl = document.getElementById('benchmarkCount');
const benchmarkBtnEl = document.getElementById('benchmarkBtn');
const benchmarkResultsEl = document.getElementById('benchmarkResults');

// State
let operationCount = 0;
let entityCount = 0;

// Simple JSON to CBOR (for demo - simplified encoding)
function jsonToCbor(json) {
    const obj = JSON.parse(json);
    return new TextEncoder().encode(JSON.stringify(obj));
}

function cborToJson(bytes) {
    return new TextDecoder().decode(bytes);
}

// Logging
function log(message, type = 'info') {
    const time = new Date().toLocaleTimeString();
    const entry = document.createElement('div');
    entry.className = 'log-entry';
    entry.innerHTML = `<span class="log-time">[${time}]</span> <span class="log-${type}">${message}</span>`;
    logEl.insertBefore(entry, logEl.firstChild);
    
    // Keep only last 50 entries
    while (logEl.children.length > 50) {
        logEl.removeChild(logEl.lastChild);
    }
}

function incrementOps() {
    operationCount++;
    opCountEl.textContent = operationCount;
}

async function updateEntityCount() {
    try {
        const items = await client.list(putCollectionEl.value);
        entityCount = items.length;
        entityCountEl.textContent = entityCount;
    } catch (e) {
        // Ignore errors during count update
    }
}

// Initialize
async function init() {
    try {
        log('Initializing worker...');
        await client.init();
        
        log('Opening database...');
        const result = await client.open();
        
        workerStatusEl.classList.add('ready');
        workerStatusTextEl.textContent = `Ready (v${result.version})`;
        log(`Database opened (version ${result.version})`, 'success');
        
        enableButtons(true);
        
    } catch (error) {
        workerStatusEl.classList.add('error');
        workerStatusTextEl.textContent = 'Error';
        log(`Initialization failed: ${error.message}`, 'error');
        console.error(error);
    }
}

function enableButtons(enabled) {
    putBtnEl.disabled = !enabled;
    putManyBtnEl.disabled = !enabled;
    getBtnEl.disabled = !enabled;
    listBtnEl.disabled = !enabled;
    benchmarkBtnEl.disabled = !enabled;
}

// Put Entity
async function handlePut() {
    try {
        let entityId = putEntityIdEl.value.trim();
        
        if (!entityId) {
            entityId = await client.generateId();
            putEntityIdEl.value = entityId;
        }
        
        const data = jsonToCbor(putDataEl.value);
        const collection = putCollectionEl.value;
        
        await client.put(collection, entityId, data);
        incrementOps();
        
        log(`Put entity ${entityId.slice(0, 8)}...`, 'success');
        await updateEntityCount();
        
    } catch (error) {
        log(`Put failed: ${error.message}`, 'error');
    }
}

// Put Many Entities
async function handlePutMany() {
    try {
        const collection = putCollectionEl.value;
        const items = [];
        
        for (let i = 0; i < 100; i++) {
            const entityId = await client.generateId();
            const data = jsonToCbor(JSON.stringify({
                name: `Item ${i}`,
                value: Math.floor(Math.random() * 1000),
                timestamp: Date.now()
            }));
            items.push({ entityId, data });
        }
        
        const result = await client.batchPut(collection, items);
        operationCount += result.count;
        opCountEl.textContent = operationCount;
        
        log(`Batch put ${result.count} entities`, 'success');
        await updateEntityCount();
        
    } catch (error) {
        log(`Batch put failed: ${error.message}`, 'error');
    }
}

// Get Entity
async function handleGet() {
    try {
        const collection = getCollectionEl.value;
        const entityId = getEntityIdEl.value.trim();
        
        if (!entityId) {
            log('Please enter an entity ID', 'error');
            return;
        }
        
        const data = await client.get(collection, entityId);
        incrementOps();
        
        if (data) {
            const json = cborToJson(data);
            getOutputEl.textContent = JSON.stringify(JSON.parse(json), null, 2);
            log(`Got entity ${entityId.slice(0, 8)}...`, 'success');
        } else {
            getOutputEl.textContent = 'Entity not found';
            log(`Entity ${entityId.slice(0, 8)}... not found`, 'info');
        }
        
    } catch (error) {
        log(`Get failed: ${error.message}`, 'error');
        getOutputEl.textContent = `Error: ${error.message}`;
    }
}

// List All Entities
async function handleList() {
    try {
        const collection = getCollectionEl.value;
        const items = await client.list(collection);
        incrementOps();
        
        if (items.length === 0) {
            getOutputEl.textContent = 'No entities in collection';
            log(`Listed ${collection}: 0 entities`, 'info');
            return;
        }
        
        const output = items.map(item => {
            const json = cborToJson(item.data);
            return `${item.id.slice(0, 8)}...: ${json}`;
        }).join('\n');
        
        getOutputEl.textContent = output;
        log(`Listed ${collection}: ${items.length} entities`, 'success');
        
    } catch (error) {
        log(`List failed: ${error.message}`, 'error');
        getOutputEl.textContent = `Error: ${error.message}`;
    }
}

// Benchmark
async function handleBenchmark() {
    const count = parseInt(benchmarkCountEl.value) || 1000;
    benchmarkBtnEl.disabled = true;
    benchmarkResultsEl.innerHTML = '<div style="color: var(--text-muted);">Running benchmark...</div>';
    
    try {
        const collection = 'benchmark';
        
        // Generate IDs
        log(`Generating ${count} entity IDs...`);
        const ids = [];
        const idStart = performance.now();
        for (let i = 0; i < count; i++) {
            ids.push(await client.generateId());
        }
        const idTime = performance.now() - idStart;
        
        // Put entities
        log(`Putting ${count} entities...`);
        const putStart = performance.now();
        for (let i = 0; i < count; i++) {
            const data = new TextEncoder().encode(`{"i":${i}}`);
            await client.put(collection, ids[i], data);
        }
        const putTime = performance.now() - putStart;
        
        // Get entities
        log(`Getting ${count} entities...`);
        const getStart = performance.now();
        for (let i = 0; i < count; i++) {
            await client.get(collection, ids[i]);
        }
        const getTime = performance.now() - getStart;
        
        // Results
        const results = [
            { label: 'ID Generation', time: idTime, ops: count / (idTime / 1000) },
            { label: 'Put Operations', time: putTime, ops: count / (putTime / 1000) },
            { label: 'Get Operations', time: getTime, ops: count / (getTime / 1000) }
        ];
        
        benchmarkResultsEl.innerHTML = results.map(r => `
            <div class="benchmark-row">
                <span>${r.label}</span>
                <span class="benchmark-value">${r.time.toFixed(1)}ms (${Math.floor(r.ops)} ops/s)</span>
            </div>
        `).join('');
        
        operationCount += count * 3;
        opCountEl.textContent = operationCount;
        
        log(`Benchmark complete: ${count * 3} operations`, 'success');
        await updateEntityCount();
        
    } catch (error) {
        benchmarkResultsEl.innerHTML = `<div style="color: var(--danger);">Error: ${error.message}</div>`;
        log(`Benchmark failed: ${error.message}`, 'error');
    } finally {
        benchmarkBtnEl.disabled = false;
    }
}

// Event Listeners
putBtnEl.addEventListener('click', handlePut);
putManyBtnEl.addEventListener('click', handlePutMany);
getBtnEl.addEventListener('click', handleGet);
listBtnEl.addEventListener('click', handleList);
benchmarkBtnEl.addEventListener('click', handleBenchmark);

// Copy entity ID on click
putEntityIdEl.addEventListener('click', async () => {
    if (putEntityIdEl.value) {
        await navigator.clipboard.writeText(putEntityIdEl.value);
        log('Entity ID copied to clipboard', 'info');
    }
});

// Start
enableButtons(false);
init();
