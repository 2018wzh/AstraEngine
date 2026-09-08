use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use crate::{
    model::{SecretResolver, TranslationError, TranslationRequest, TranslationResult},
    prompt::prompt_segment_count,
    transport::OpenAiCompatibleTranslationProvider,
    DEFAULT_CACHE_CHARS, DEFAULT_CACHE_ENTRIES,
};

#[derive(Debug, Clone)]
struct CacheEntry {
    current_text: String,
    translated: String,
    char_cost: usize,
}

/// FIFO cache scoped to one live game session. It intentionally has no
/// serialization or disk interface.
#[derive(Debug)]
pub struct TranslationSessionCache {
    entries: VecDeque<CacheEntry>,
    chars: usize,
    max_entries: usize,
    max_chars: usize,
}

impl Default for TranslationSessionCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_ENTRIES, DEFAULT_CACHE_CHARS)
    }
}

impl TranslationSessionCache {
    pub fn new(max_entries: usize, max_chars: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            chars: 0,
            max_entries,
            max_chars,
        }
    }

    pub fn get(&self, current_text: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| entry.current_text == current_text)
            .map(|entry| entry.translated.as_str())
    }

    pub fn insert(&mut self, current_text: String, translated: String) {
        let char_cost = current_text
            .chars()
            .count()
            .checked_add(translated.chars().count());
        let Some(char_cost) = char_cost else {
            return;
        };
        if self.max_entries == 0 || char_cost > self.max_chars {
            return;
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.current_text == current_text)
        {
            if let Some(old) = self.entries.remove(index) {
                self.chars = self.chars.saturating_sub(old.char_cost);
            }
        }
        while self.entries.len() >= self.max_entries
            || self.chars.saturating_add(char_cost) > self.max_chars
        {
            let Some(old) = self.entries.pop_front() else {
                break;
            };
            self.chars = self.chars.saturating_sub(old.char_cost);
        }
        self.chars = self.chars.saturating_add(char_cost);
        self.entries.push_back(CacheEntry {
            current_text,
            translated,
            char_cost,
        });
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.chars = 0;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

struct SessionCacheState {
    cache: TranslationSessionCache,
    generation: u64,
}

/// Provider plus bounded, non-persistent cache for one active game.
pub struct TranslationSession<R: ?Sized> {
    provider: Arc<OpenAiCompatibleTranslationProvider<R>>,
    state: Mutex<SessionCacheState>,
}

impl<R: SecretResolver + ?Sized> TranslationSession<R> {
    pub fn new(provider: Arc<OpenAiCompatibleTranslationProvider<R>>) -> Self {
        Self {
            provider,
            state: Mutex::new(SessionCacheState {
                cache: TranslationSessionCache::default(),
                generation: 0,
            }),
        }
    }

    pub async fn translate(
        &self,
        request: &TranslationRequest,
    ) -> Result<TranslationResult, TranslationError> {
        let expected_generation = {
            let state = self.state.lock().expect("translation cache mutex poisoned");
            state.generation
        };
        self.translate_at_generation(request, expected_generation)
            .await
    }

    pub(crate) fn generation(&self) -> u64 {
        self.state
            .lock()
            .expect("translation cache mutex poisoned")
            .generation
    }

    /// Translate only when the caller still owns the captured session
    /// generation. The generation is checked before reading the cache and
    /// again before publishing a provider result so a request that starts
    /// after a reset cannot read or populate the new game's cache.
    pub(crate) async fn translate_at_generation(
        &self,
        request: &TranslationRequest,
        expected_generation: u64,
    ) -> Result<TranslationResult, TranslationError> {
        request.validate()?;
        {
            let state = self.state.lock().expect("translation cache mutex poisoned");
            if state.generation != expected_generation {
                return Err(TranslationError::SessionReset);
            }
            if let Some(translated) = state.cache.get(&request.current.text) {
                return Ok(TranslationResult {
                    translated: translated.to_owned(),
                    provider_identity: self.provider.provider_identity(),
                    latency_ms: 0,
                    sent_segment_count: prompt_segment_count(request),
                    cache_hit: true,
                });
            }
        }
        let result = self.provider.translate(request).await?;
        let mut state = self.state.lock().expect("translation cache mutex poisoned");
        if state.generation != expected_generation {
            return Err(TranslationError::SessionReset);
        }
        state
            .cache
            .insert(request.current.text.clone(), result.translated.clone());
        Ok(result)
    }

    pub fn clear(&self) {
        let mut state = self.state.lock().expect("translation cache mutex poisoned");
        state.generation = state
            .generation
            .checked_add(1)
            .expect("translation session generation exhausted");
        state.cache.clear();
    }

    pub fn clear_for_new_game(&self) {
        self.clear();
    }

    pub fn clear_for_load(&self) {
        self.clear();
    }

    pub fn clear_for_configuration_change(&self) {
        self.clear();
    }

    pub fn cache_len(&self) -> usize {
        self.state
            .lock()
            .expect("translation cache mutex poisoned")
            .cache
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_is_bounded_and_clears_without_serialization() {
        let mut cache = TranslationSessionCache::new(2, 6);
        cache.insert("a".into(), "aa".into());
        cache.insert("b".into(), "bb".into());
        cache.insert("c".into(), "cc".into());
        assert_eq!(cache.len(), 2);
        assert!(cache.get("a").is_none());
        assert_eq!(cache.get("c"), Some("cc"));
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_character_budget_counts_source_and_translation() {
        let mut cache = TranslationSessionCache::new(8, 5);
        cache.insert("source".into(), "x".into());
        assert!(cache.is_empty());

        cache.insert("ab".into(), "cde".into());
        assert_eq!(cache.get("ab"), Some("cde"));

        cache.insert("f".into(), "gh".into());
        assert!(cache.get("ab").is_none());
        assert_eq!(cache.get("f"), Some("gh"));
    }
}
