# EntiDB Web Worker Example

Demonstrates running EntiDB in a Web Worker for non-blocking database operations.

## Why Use a Web Worker?

- **Non-blocking UI** - Database operations don't freeze the main thread
- **Better Performance** - Heavy operations run in parallel
- **OPFS Sync Access** - Web Workers can use synchronous OPFS file handles

## Architecture

```
┌─────────────────┐         Messages         ┌─────────────────┐
│   Main Thread   │  ◄─────────────────────►  │   Web Worker    │
│                 │                           │                 │
│  UI / Events    │                           │  EntiDB WASM    │
│  DOM Updates    │                           │  Database Ops   │
└─────────────────┘                           └─────────────────┘
```

## Running

1. Build the WASM package:

```bash
cd ../../entidb_wasm
wasm-pack build --target web --out-dir ../examples/worker/pkg
```

2. Install dependencies and run:

```bash
npm install
npm run dev
```

## Files

- `index.html` - Main UI
- `main.js` - Main thread code (UI + message handling)
- `worker.js` - Web Worker with EntiDB
- `shared.js` - Shared message types

## Message Protocol

The main thread and worker communicate via structured messages:

```javascript
// Request
{ type: 'put', id: requestId, payload: { collection, entityId, data } }

// Response
{ type: 'put', id: requestId, success: true, result: ... }
{ type: 'put', id: requestId, success: false, error: '...' }
```

## Notes

- All database operations are async from the main thread's perspective
- The worker can be extended to handle batch operations
- Future versions will use SharedArrayBuffer for even faster communication
