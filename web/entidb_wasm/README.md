# EntiDB WASM

EntiDB WebAssembly bindings with OPFS and IndexedDB storage backends.

## Overview

This package provides WebAssembly bindings for EntiDB, enabling the embedded entity 
database engine to run in web browsers with persistent storage via OPFS or IndexedDB.

## Features

- **Full EntiDB Core**: Complete database engine compiled to WASM
- **OPFS Backend**: Uses Origin Private File System for efficient file-like storage
- **IndexedDB Fallback**: Falls back to IndexedDB when OPFS is not available
- **ACID Transactions**: Same transactional guarantees as native
- **Zero Dependencies**: No external database required

## Installation

```bash
npm install @entidb/wasm
```

## Usage

```javascript
import init, { Database } from '@entidb/wasm';

async function main() {
  // Initialize WASM module
  await init();
  
  // Open a database with OPFS storage
  const db = await Database.openOpfs('my-database');
  
  // Get or create a collection
  const users = db.collection('users');
  
  // Store data
  const userId = db.generateId();
  db.put(users, userId, new TextEncoder().encode('{"name": "Alice"}'));
  
  // Retrieve data
  const data = db.get(users, userId);
  console.log(new TextDecoder().decode(data));
  
  // Close the database
  db.close();
}

main();
```

## Storage Backends

### OPFS (Recommended)

OPFS provides file-system-like APIs for efficient storage:

```javascript
const db = await Database.openOpfs('my-database');
```

### IndexedDB

Fallback when OPFS is not available:

```javascript
const db = await Database.openIndexedDb('my-database');
```

### In-Memory

For testing or temporary data:

```javascript
const db = Database.openMemory();
```

## Browser Compatibility

| Browser | OPFS | IndexedDB |
|---------|------|-----------|
| Chrome 86+ | ✅ | ✅ |
| Firefox 111+ | ✅ | ✅ |
| Safari 15.2+ | ✅ | ✅ |
| Edge 86+ | ✅ | ✅ |

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
