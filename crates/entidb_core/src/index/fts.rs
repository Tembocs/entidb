//! Full-text index implementation.
//!
//! The `FtsIndex` provides token-based text search with support for:
//! - Tokenization (whitespace, punctuation splitting)
//! - Case-insensitive matching
//! - Prefix matching
//! - Multi-token queries (AND semantics)
//!
//! ## Phase 2 Feature
//!
//! This is a Phase 2 feature per the architecture docs. It provides
//! token-based exact match; no ranking or fuzzy matching in this phase.

use crate::entity::EntityId;
use crate::error::CoreResult;
use crate::types::CollectionId;
use std::collections::{HashMap, HashSet};

/// Configuration for the FTS tokenizer.
#[derive(Debug, Clone)]
pub struct TokenizerConfig {
    /// Minimum token length to index.
    pub min_token_length: usize,
    /// Maximum token length to index.
    pub max_token_length: usize,
    /// Whether to perform case-insensitive matching.
    pub case_insensitive: bool,
    /// Additional characters to treat as separators.
    pub extra_separators: Vec<char>,
}

impl Default for TokenizerConfig {
    fn default() -> Self {
        Self {
            min_token_length: 1,
            max_token_length: 256,
            case_insensitive: true,
            extra_separators: vec![],
        }
    }
}

impl TokenizerConfig {
    /// Creates a new tokenizer configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets minimum token length.
    #[must_use]
    pub fn min_length(mut self, len: usize) -> Self {
        self.min_token_length = len;
        self
    }

    /// Sets maximum token length.
    #[must_use]
    pub fn max_length(mut self, len: usize) -> Self {
        self.max_token_length = len;
        self
    }

    /// Sets case sensitivity.
    #[must_use]
    pub fn case_sensitive(mut self) -> Self {
        self.case_insensitive = false;
        self
    }

    /// Adds extra separator characters.
    #[must_use]
    #[allow(dead_code)] // Public API for bindings
    pub fn with_separators(mut self, chars: &[char]) -> Self {
        self.extra_separators.extend_from_slice(chars);
        self
    }
}

/// Specification for a full-text index.
#[derive(Debug, Clone)]
pub struct FtsIndexSpec {
    /// Collection this index belongs to.
    pub collection_id: CollectionId,
    /// Name of the index (internal, not user-facing).
    pub name: String,
    /// Tokenizer configuration.
    pub tokenizer: TokenizerConfig,
}

impl FtsIndexSpec {
    /// Creates a new FTS index specification.
    pub fn new(collection_id: CollectionId, name: impl Into<String>) -> Self {
        Self {
            collection_id,
            name: name.into(),
            tokenizer: TokenizerConfig::default(),
        }
    }

    /// Sets the tokenizer configuration.
    #[must_use]
    pub fn with_tokenizer(mut self, config: TokenizerConfig) -> Self {
        self.tokenizer = config;
        self
    }
}

/// Full-text index for token-based text search.
///
/// `FtsIndex` provides:
/// - Inverted index: token → set of entity IDs
/// - Forward index: entity ID → set of tokens (for updates)
/// - Case-insensitive matching (configurable)
/// - Prefix search
/// - Multi-token AND queries
///
/// # Example
///
/// ```rust,ignore
/// let spec = FtsIndexSpec::new(CollectionId::new(1), "content_fts");
/// let mut index = FtsIndex::new(spec);
///
/// // Index a document
/// index.index_text(entity_id, "Hello world, this is a test")?;
///
/// // Search
/// let results = index.search("hello")?;
/// ```
pub struct FtsIndex {
    /// Index specification.
    spec: FtsIndexSpec,
    /// Inverted index: normalized token → set of entity IDs.
    inverted: HashMap<String, HashSet<EntityId>>,
    /// Forward index: entity ID → set of indexed tokens.
    forward: HashMap<EntityId, HashSet<String>>,
    /// Total token count.
    token_count: usize,
}

#[allow(dead_code)] // Many methods are public API for bindings
impl FtsIndex {
    /// Creates a new full-text index.
    pub fn new(spec: FtsIndexSpec) -> Self {
        Self {
            spec,
            inverted: HashMap::new(),
            forward: HashMap::new(),
            token_count: 0,
        }
    }

    /// Returns the index specification.
    pub fn spec(&self) -> &FtsIndexSpec {
        &self.spec
    }

    /// Returns the number of indexed tokens (unique tokens × entity references).
    pub fn token_count(&self) -> usize {
        self.token_count
    }

    /// Returns the number of unique tokens in the index.
    pub fn unique_token_count(&self) -> usize {
        self.inverted.len()
    }

    /// Returns the number of indexed entities.
    pub fn entity_count(&self) -> usize {
        self.forward.len()
    }

    /// Returns true if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }

    /// Clears the index.
    pub fn clear(&mut self) {
        self.inverted.clear();
        self.forward.clear();
        self.token_count = 0;
    }

    /// Tokenizes text according to the configuration.
    fn tokenize(&self, text: &str) -> Vec<String> {
        let config = &self.spec.tokenizer;
        let mut tokens = Vec::new();
        let mut current_token = String::new();

        for c in text.chars() {
            let is_separator = c.is_whitespace()
                || c.is_ascii_punctuation()
                || config.extra_separators.contains(&c);

            if is_separator {
                if !current_token.is_empty() {
                    if current_token.len() >= config.min_token_length
                        && current_token.len() <= config.max_token_length
                    {
                        let normalized = if config.case_insensitive {
                            current_token.to_lowercase()
                        } else {
                            current_token.clone()
                        };
                        tokens.push(normalized);
                    }
                    current_token.clear();
                }
            } else {
                current_token.push(c);
            }
        }

        // Don't forget the last token
        if !current_token.is_empty()
            && current_token.len() >= config.min_token_length
            && current_token.len() <= config.max_token_length
        {
            let normalized = if config.case_insensitive {
                current_token.to_lowercase()
            } else {
                current_token
            };
            tokens.push(normalized);
        }

        tokens
    }

    /// Indexes text for an entity.
    ///
    /// This replaces any previously indexed text for the entity.
    pub fn index_text(&mut self, entity_id: EntityId, text: &str) -> CoreResult<()> {
        // Remove old tokens if entity was previously indexed
        self.remove_entity(entity_id)?;

        // Tokenize the new text
        let tokens = self.tokenize(text);
        let token_set: HashSet<String> = tokens.into_iter().collect();

        // Add to inverted index
        for token in &token_set {
            let entry = self.inverted.entry(token.clone()).or_default();
            entry.insert(entity_id);
            self.token_count += 1;
        }

        // Add to forward index
        self.forward.insert(entity_id, token_set);

        Ok(())
    }

    /// Removes an entity from the index.
    pub fn remove_entity(&mut self, entity_id: EntityId) -> CoreResult<bool> {
        let Some(tokens) = self.forward.remove(&entity_id) else {
            return Ok(false);
        };

        // Remove from inverted index
        for token in &tokens {
            if let Some(entities) = self.inverted.get_mut(token) {
                entities.remove(&entity_id);
                self.token_count = self.token_count.saturating_sub(1);

                // Clean up empty entries
                if entities.is_empty() {
                    self.inverted.remove(token);
                }
            }
        }

        Ok(true)
    }

    /// Searches for entities matching a single token (exact match).
    pub fn search_token(&self, token: &str) -> CoreResult<Vec<EntityId>> {
        let normalized = if self.spec.tokenizer.case_insensitive {
            token.to_lowercase()
        } else {
            token.to_string()
        };

        match self.inverted.get(&normalized) {
            Some(entities) => Ok(entities.iter().copied().collect()),
            None => Ok(Vec::new()),
        }
    }

    /// Searches for entities matching a prefix.
    ///
    /// Returns entities containing any token that starts with the given prefix.
    pub fn search_prefix(&self, prefix: &str) -> CoreResult<Vec<EntityId>> {
        let normalized = if self.spec.tokenizer.case_insensitive {
            prefix.to_lowercase()
        } else {
            prefix.to_string()
        };

        let mut results = HashSet::new();

        for (token, entities) in &self.inverted {
            if token.starts_with(&normalized) {
                results.extend(entities.iter().copied());
            }
        }

        Ok(results.into_iter().collect())
    }

    /// Searches for entities matching all tokens in the query (AND semantics).
    ///
    /// The query is tokenized the same way as indexed text.
    pub fn search(&self, query: &str) -> CoreResult<Vec<EntityId>> {
        let query_tokens = self.tokenize(query);

        if query_tokens.is_empty() {
            return Ok(Vec::new());
        }

        // Get entities for first token
        let mut results: HashSet<EntityId> = match self.inverted.get(&query_tokens[0]) {
            Some(entities) => entities.clone(),
            None => return Ok(Vec::new()),
        };

        // Intersect with entities for remaining tokens
        for token in query_tokens.iter().skip(1) {
            match self.inverted.get(token) {
                Some(entities) => {
                    results.retain(|id| entities.contains(id));
                    if results.is_empty() {
                        return Ok(Vec::new());
                    }
                }
                None => return Ok(Vec::new()),
            }
        }

        Ok(results.into_iter().collect())
    }

    /// Searches with OR semantics (returns entities matching any token).
    pub fn search_any(&self, query: &str) -> CoreResult<Vec<EntityId>> {
        let query_tokens = self.tokenize(query);

        if query_tokens.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = HashSet::new();

        for token in &query_tokens {
            if let Some(entities) = self.inverted.get(token) {
                results.extend(entities.iter().copied());
            }
        }

        Ok(results.into_iter().collect())
    }

    /// Returns all tokens indexed for an entity.
    pub fn tokens_for_entity(&self, entity_id: EntityId) -> Option<&HashSet<String>> {
        self.forward.get(&entity_id)
    }

    /// Checks if a token exists in the index.
    pub fn contains_token(&self, token: &str) -> bool {
        let normalized = if self.spec.tokenizer.case_insensitive {
            token.to_lowercase()
        } else {
            token.to_string()
        };
        self.inverted.contains_key(&normalized)
    }

    /// Returns the count of entities containing a token.
    pub fn token_frequency(&self, token: &str) -> usize {
        let normalized = if self.spec.tokenizer.case_insensitive {
            token.to_lowercase()
        } else {
            token.to_string()
        };
        self.inverted.get(&normalized).map_or(0, |e| e.len())
    }

    /// Rebuilds the index from an iterator of (entity_id, text) pairs.
    pub fn rebuild<I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = (EntityId, String)>,
    {
        self.clear();
        for (entity_id, text) in entries {
            let _ = self.index_text(entity_id, &text);
        }
    }
}

impl std::fmt::Debug for FtsIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FtsIndex")
            .field("name", &self.spec.name)
            .field("collection_id", &self.spec.collection_id)
            .field("entity_count", &self.entity_count())
            .field("unique_tokens", &self.unique_token_count())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_index() -> FtsIndex {
        let spec = FtsIndexSpec::new(CollectionId::new(1), "test_fts");
        FtsIndex::new(spec)
    }

    fn entity(n: u8) -> EntityId {
        EntityId::from_bytes([n; 16])
    }

    #[test]
    fn tokenize_basic() {
        let index = create_index();
        let tokens = index.tokenize("Hello World");
        assert_eq!(tokens, vec!["hello", "world"]);
    }

    #[test]
    fn tokenize_with_punctuation() {
        let index = create_index();
        let tokens = index.tokenize("Hello, World! How are you?");
        assert_eq!(tokens, vec!["hello", "world", "how", "are", "you"]);
    }

    #[test]
    fn tokenize_case_insensitive() {
        let index = create_index();
        let tokens = index.tokenize("HELLO World hElLo");
        assert_eq!(tokens, vec!["hello", "world", "hello"]);
    }

    #[test]
    fn tokenize_case_sensitive() {
        let spec = FtsIndexSpec::new(CollectionId::new(1), "test")
            .with_tokenizer(TokenizerConfig::new().case_sensitive());
        let index = FtsIndex::new(spec);
        let tokens = index.tokenize("Hello World HELLO");
        assert_eq!(tokens, vec!["Hello", "World", "HELLO"]);
    }

    #[test]
    fn tokenize_min_length() {
        let spec = FtsIndexSpec::new(CollectionId::new(1), "test")
            .with_tokenizer(TokenizerConfig::new().min_length(3));
        let index = FtsIndex::new(spec);
        let tokens = index.tokenize("I am a robot");
        assert_eq!(tokens, vec!["robot"]); // only "robot" has 3+ chars
    }

    #[test]
    fn index_and_search() {
        let mut index = create_index();

        index.index_text(entity(1), "Hello world").unwrap();
        index.index_text(entity(2), "World of rust").unwrap();
        index.index_text(entity(3), "Rust is great").unwrap();

        let results = index.search("world").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.contains(&entity(1)));
        assert!(results.contains(&entity(2)));

        let results = index.search("rust").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.contains(&entity(2)));
        assert!(results.contains(&entity(3)));
    }

    #[test]
    fn search_multi_token() {
        let mut index = create_index();

        index.index_text(entity(1), "Hello world").unwrap();
        index.index_text(entity(2), "World of rust").unwrap();
        index.index_text(entity(3), "Hello rust").unwrap();

        // AND semantics: must match both tokens
        let results = index.search("hello rust").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entity(3)));
    }

    #[test]
    fn search_any() {
        let mut index = create_index();

        index.index_text(entity(1), "Hello world").unwrap();
        index.index_text(entity(2), "World of rust").unwrap();
        index.index_text(entity(3), "Hello rust").unwrap();

        // OR semantics: match any token
        let results = index.search_any("hello rust").unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn search_prefix() {
        let mut index = create_index();

        index.index_text(entity(1), "rust").unwrap();
        index.index_text(entity(2), "rusty").unwrap();
        index.index_text(entity(3), "ruby").unwrap();

        let results = index.search_prefix("rus").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.contains(&entity(1)));
        assert!(results.contains(&entity(2)));
    }

    #[test]
    fn remove_entity() {
        let mut index = create_index();

        index.index_text(entity(1), "Hello world").unwrap();
        index.index_text(entity(2), "World of rust").unwrap();

        let removed = index.remove_entity(entity(1)).unwrap();
        assert!(removed);

        // "hello" should no longer return results
        let results = index.search("hello").unwrap();
        assert!(results.is_empty());

        // "world" should still find entity(2)
        let results = index.search("world").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entity(2)));
    }

    #[test]
    fn update_entity() {
        let mut index = create_index();

        index.index_text(entity(1), "Hello world").unwrap();

        // Re-indexing the same entity replaces the old text
        index.index_text(entity(1), "Goodbye world").unwrap();

        // "hello" should no longer match
        let results = index.search("hello").unwrap();
        assert!(results.is_empty());

        // "goodbye" should match
        let results = index.search("goodbye").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn token_frequency() {
        let mut index = create_index();

        index.index_text(entity(1), "rust rust rust").unwrap();
        index.index_text(entity(2), "rust is great").unwrap();
        index.index_text(entity(3), "rust programming").unwrap();

        // "rust" appears in 3 entities
        assert_eq!(index.token_frequency("rust"), 3);
        // "great" appears in 1 entity
        assert_eq!(index.token_frequency("great"), 1);
        // "python" appears in 0 entities
        assert_eq!(index.token_frequency("python"), 0);
    }

    #[test]
    fn tokens_for_entity() {
        let mut index = create_index();

        index.index_text(entity(1), "Hello World").unwrap();

        let tokens = index.tokens_for_entity(entity(1)).unwrap();
        assert!(tokens.contains("hello"));
        assert!(tokens.contains("world"));
        assert!(!tokens.contains("foo"));
    }

    #[test]
    fn clear_index() {
        let mut index = create_index();

        index.index_text(entity(1), "Hello world").unwrap();
        index.index_text(entity(2), "World of rust").unwrap();

        assert_eq!(index.entity_count(), 2);

        index.clear();

        assert!(index.is_empty());
        assert_eq!(index.entity_count(), 0);
        assert_eq!(index.unique_token_count(), 0);
    }

    #[test]
    fn empty_query() {
        let mut index = create_index();
        index.index_text(entity(1), "Hello world").unwrap();

        let results = index.search("").unwrap();
        assert!(results.is_empty());

        let results = index.search("   ").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn no_match() {
        let mut index = create_index();
        index.index_text(entity(1), "Hello world").unwrap();

        let results = index.search("rust").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn rebuild() {
        let mut index = create_index();

        index.index_text(entity(1), "old text").unwrap();

        index.rebuild(vec![
            (entity(2), "new text".to_string()),
            (entity(3), "another text".to_string()),
        ]);

        assert_eq!(index.entity_count(), 2);
        assert!(index.search("old").unwrap().is_empty());
        assert_eq!(index.search("new").unwrap().len(), 1);
    }

    #[test]
    fn unicode_text() {
        let mut index = create_index();

        index.index_text(entity(1), "こんにちは世界").unwrap();
        index.index_text(entity(2), "Привет мир").unwrap();
        index.index_text(entity(3), "مرحبا بالعالم").unwrap();

        // Should be able to search unicode
        let results = index.search("こんにちは世界").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&entity(1)));
    }
}
