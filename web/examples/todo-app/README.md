# EntiDB Todo App Example

A practical todo application demonstrating EntiDB usage in a web browser.

## Features

- **Full CRUD operations** - Create, complete, and delete todos
- **Persistent-like state** - Data persists across interactions (in-memory)
- **Real-time updates** - UI updates immediately after database operations
- **CBOR encoding** - Uses proper CBOR map encoding for structured data
- **Filtering** - Filter by all, active, or completed todos

## Running

1. Build the WASM package:

```bash
cd ../../entidb_wasm
wasm-pack build --target web --out-dir ../examples/todo-app/pkg
```

2. Install dependencies and run:

```bash
npm install
npm run dev
```

## Code Structure

- `index.html` - Todo app UI with TodoMVC-inspired styling
- `app.js` - Main application logic
- `cbor.js` - CBOR encoding/decoding utilities
- `store.js` - EntiDB store wrapper

## Data Model

Each todo is stored as a CBOR map:

```javascript
{
  title: "string",
  completed: false,
  createdAt: 1234567890
}
```
