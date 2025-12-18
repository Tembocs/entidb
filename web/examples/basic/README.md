# EntiDB Web Example

This example demonstrates using EntiDB in a web browser via WebAssembly.

## Prerequisites

1. Install [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/)
2. Install Node.js (v18+)

## Building

First, build the WASM package:

```bash
cd ../../entidb_wasm
wasm-pack build --target web --out-dir ../examples/basic/pkg
```

Or use the npm script:

```bash
npm run build:wasm
```

## Running

Install dependencies:

```bash
npm install
```

Start the development server:

```bash
npm run dev
```

This will open the example in your default browser at http://localhost:3000.

## Features Demonstrated

- **In-memory database**: Creates an EntiDB database in browser memory
- **CRUD operations**: Create, read, update, delete entities
- **Collections**: Uses the "users" collection to store entities
- **CBOR encoding**: Entities are stored as CBOR-encoded data
- **Entity IDs**: Demonstrates generating and displaying entity IDs

## Code Structure

- `index.html` - The HTML UI
- `main.js` - JavaScript code that interacts with EntiDB WASM
- `pkg/` - Built WASM package (generated)
- `vite.config.js` - Vite dev server configuration

## Notes

- This example uses an in-memory database. Data is lost when the page is refreshed.
- Future versions will support persistent storage via OPFS or IndexedDB.
- The CBOR encoding in this example is simplified. For production, use a proper CBOR library.
