# EntiDB Notes App - Persistent Storage Example

This example demonstrates EntiDB's **persistent storage** capabilities using the browser's Origin Private File System (OPFS) or IndexedDB as a fallback.

## Features

- **Persistent Storage**: Notes survive page refreshes and browser restarts
- **Automatic Storage Detection**: Uses OPFS when available, falls back to IndexedDB/localStorage
- **Real-time Status**: Shows storage type and sync status
- **Markdown Support**: Notes support basic formatting
- **Search**: Filter notes by title or content

## Running the Example

1. First, build the WASM package:
   ```bash
   cd ../../entidb_wasm
   wasm-pack build --target web
   ```

2. Install dependencies:
   ```bash
   npm install
   ```

3. Run the development server:
   ```bash
   npm run dev
   ```

4. Open http://localhost:5173 in your browser.

## Storage Types

| Storage Type | Persistence | Browser Support |
|--------------|-------------|-----------------|
| OPFS | ✅ Full | Chrome 86+, Edge 86+, Firefox 111+ |
| IndexedDB | ✅ Full | All modern browsers |
| localStorage | ⚠️ Limited (5MB) | All browsers |

## Code Structure

- `index.html` - Main HTML with embedded styles
- `app.js` - Application logic and UI management
- `store.js` - EntiDB wrapper for persistent storage
- `cbor.js` - CBOR encoding/decoding utilities

## Notes on Persistence

The example automatically:
1. Opens a persistent database named "notes-app"
2. Saves changes on every modification
3. Loads existing notes on page load
4. Shows unsaved changes indicator
