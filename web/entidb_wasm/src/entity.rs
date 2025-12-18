//! Entity types for WASM bindings.

use entidb_core::EntityId as CoreEntityId;
use wasm_bindgen::prelude::*;

/// A unique identifier for an entity.
///
/// EntityIds are 128-bit UUIDs that uniquely identify entities within
/// a database. They are stable and immutable once created.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityId(CoreEntityId);

#[wasm_bindgen]
impl EntityId {
    /// Generates a new random EntityId.
    ///
    /// Each call produces a unique identifier using UUID v4.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(CoreEntityId::new())
    }

    /// Generates a new random EntityId.
    ///
    /// Alias for the constructor.
    #[wasm_bindgen]
    pub fn generate() -> Self {
        Self::new()
    }

    /// Creates an EntityId from raw bytes.
    ///
    /// # Arguments
    ///
    /// * `bytes` - A 16-byte array containing the UUID bytes
    ///
    /// # Errors
    ///
    /// Returns an error if the byte array is not exactly 16 bytes.
    #[wasm_bindgen(js_name = fromBytes)]
    pub fn from_bytes(bytes: &[u8]) -> Result<EntityId, JsValue> {
        if bytes.len() != 16 {
            return Err(JsValue::from_str("EntityId requires exactly 16 bytes"));
        }
        let mut arr = [0u8; 16];
        arr.copy_from_slice(bytes);
        Ok(Self(CoreEntityId::from_bytes(arr)))
    }

    /// Returns the raw bytes of this EntityId.
    #[wasm_bindgen(js_name = toBytes)]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.as_bytes().to_vec()
    }

    /// Returns a hexadecimal string representation.
    #[wasm_bindgen(js_name = toHex)]
    pub fn to_hex(&self) -> String {
        self.0
            .as_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }

    /// Creates an EntityId from a hexadecimal string.
    ///
    /// # Arguments
    ///
    /// * `hex` - A 32-character hexadecimal string
    ///
    /// # Errors
    ///
    /// Returns an error if the string is not valid hex or wrong length.
    #[wasm_bindgen(js_name = fromHex)]
    pub fn from_hex(hex: &str) -> Result<EntityId, JsValue> {
        if hex.len() != 32 {
            return Err(JsValue::from_str("Hex string must be 32 characters"));
        }

        let bytes: Result<Vec<u8>, _> = (0..16)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16))
            .collect();

        match bytes {
            Ok(b) => Self::from_bytes(&b),
            Err(_) => Err(JsValue::from_str("Invalid hex string")),
        }
    }

    /// Returns a string representation for debugging.
    #[wasm_bindgen(js_name = toString)]
    pub fn to_string_js(&self) -> String {
        format!("EntityId({})", self.to_hex())
    }

    /// Checks equality with another EntityId.
    #[wasm_bindgen]
    pub fn equals(&self, other: &EntityId) -> bool {
        self.0 == other.0
    }
}

impl Default for EntityId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<CoreEntityId> for EntityId {
    fn from(id: CoreEntityId) -> Self {
        Self(id)
    }
}

impl From<EntityId> for CoreEntityId {
    fn from(id: EntityId) -> Self {
        id.0
    }
}

/// A reference to a collection in the database.
///
/// Collections group related entities together. Each entity belongs
/// to exactly one collection.
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct Collection {
    name: String,
    id: u32,
}

#[wasm_bindgen]
impl Collection {
    /// Creates a new collection reference.
    #[wasm_bindgen(constructor)]
    pub fn new(name: String, id: u32) -> Self {
        Self { name, id }
    }

    /// Returns the collection name.
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Returns the collection ID.
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> u32 {
        self.id
    }
}
