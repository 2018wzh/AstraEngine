use std::collections::BTreeMap;

use astra_ui_core::{UiValidationError, MAX_NODES_PER_VIEW};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRange {
    pub start: usize,
    pub end: usize,
}

impl VisibleRange {
    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Debug, Clone)]
pub struct VirtualListState {
    extents: Vec<f32>,
    prefix: Vec<f32>,
    estimated_extent: f32,
    viewport_extent: f32,
    scroll_offset: f32,
    overscan: usize,
    dirty_from: usize,
}

impl VirtualListState {
    pub fn new(
        item_count: usize,
        estimated_extent: f32,
        viewport_extent: f32,
        overscan: usize,
    ) -> Result<Self, UiValidationError> {
        if item_count > 1_000_000 {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_VIRTUAL_ITEM_LIMIT",
                "virtual collection item count exceeds 1,000,000",
            ));
        }
        validate_extent("estimated item extent", estimated_extent)?;
        validate_extent("viewport extent", viewport_extent)?;
        if overscan > MAX_NODES_PER_VIEW {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_VIRTUAL_OVERSCAN",
                "virtual collection overscan exceeds node budget",
            ));
        }
        let extents = vec![estimated_extent; item_count];
        let mut state = Self {
            prefix: vec![0.0; item_count + 1],
            extents,
            estimated_extent,
            viewport_extent,
            scroll_offset: 0.0,
            overscan,
            dirty_from: 0,
        };
        state.rebuild_prefix();
        Ok(state)
    }

    pub fn set_item_count(&mut self, item_count: usize) -> Result<(), UiValidationError> {
        if item_count > 1_000_000 {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_VIRTUAL_ITEM_LIMIT",
                "virtual collection item count exceeds 1,000,000",
            ));
        }
        self.extents.resize(item_count, self.estimated_extent);
        self.prefix.resize(item_count + 1, 0.0);
        self.dirty_from = self.dirty_from.min(item_count);
        self.rebuild_prefix();
        self.set_scroll_offset(self.scroll_offset)?;
        Ok(())
    }

    pub fn set_viewport_extent(&mut self, viewport_extent: f32) -> Result<(), UiValidationError> {
        validate_extent("viewport extent", viewport_extent)?;
        self.viewport_extent = viewport_extent;
        self.set_scroll_offset(self.scroll_offset)
    }

    pub fn set_scroll_offset(&mut self, offset: f32) -> Result<(), UiValidationError> {
        if !offset.is_finite() {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_VIRTUAL_SCROLL",
                "virtual collection scroll offset must be finite",
            ));
        }
        self.rebuild_prefix();
        self.scroll_offset = offset.clamp(0.0, self.max_scroll_offset());
        Ok(())
    }

    pub fn scroll_by(&mut self, delta: f32) -> Result<(), UiValidationError> {
        self.set_scroll_offset(self.scroll_offset + delta)
    }

    pub fn update_measurement(
        &mut self,
        index: usize,
        extent: f32,
    ) -> Result<(), UiValidationError> {
        validate_extent("measured item extent", extent)?;
        let item = self.extents.get_mut(index).ok_or_else(|| {
            UiValidationError::invalid(
                "ASTRA_UI_VIRTUAL_INDEX",
                "measured item index is outside the collection",
            )
        })?;
        if (*item - extent).abs() > f32::EPSILON {
            *item = extent;
            self.dirty_from = self.dirty_from.min(index);
        }
        Ok(())
    }

    pub fn visible_range(&mut self) -> VisibleRange {
        self.rebuild_prefix();
        if self.extents.is_empty() {
            return VisibleRange { start: 0, end: 0 };
        }
        let visible_start = self.partition_offset(self.scroll_offset);
        let visible_end = self
            .partition_offset(self.scroll_offset + self.viewport_extent)
            .saturating_add(1)
            .min(self.extents.len());
        VisibleRange {
            start: visible_start.saturating_sub(self.overscan),
            end: visible_end
                .saturating_add(self.overscan)
                .min(self.extents.len()),
        }
    }

    pub fn item_offset(&mut self, index: usize) -> Option<f32> {
        self.rebuild_prefix();
        self.prefix.get(index).copied()
    }

    pub fn total_extent(&mut self) -> f32 {
        self.rebuild_prefix();
        self.prefix.last().copied().unwrap_or(0.0)
    }

    pub fn max_scroll_offset(&mut self) -> f32 {
        (self.total_extent() - self.viewport_extent).max(0.0)
    }

    pub fn instantiated_count(&mut self) -> usize {
        self.visible_range().len()
    }

    pub fn visible_leading_extent(&mut self, range: VisibleRange) -> f32 {
        self.item_offset(range.start)
            .map_or(0.0, |offset| (offset - self.scroll_offset).max(0.0))
    }

    fn rebuild_prefix(&mut self) {
        if self.dirty_from >= self.extents.len() && self.prefix.len() == self.extents.len() + 1 {
            return;
        }
        let start = self.dirty_from.min(self.extents.len());
        if start == 0 {
            self.prefix[0] = 0.0;
        }
        for index in start..self.extents.len() {
            self.prefix[index + 1] = self.prefix[index] + self.extents[index];
        }
        self.dirty_from = self.extents.len();
    }

    fn partition_offset(&self, offset: f32) -> usize {
        self.prefix
            .partition_point(|value| *value <= offset)
            .saturating_sub(1)
            .min(self.extents.len().saturating_sub(1))
    }
}

#[derive(Debug, Clone)]
pub struct VirtualGridState {
    list: VirtualListState,
    item_count: usize,
    columns: usize,
}

impl VirtualGridState {
    pub fn new(
        item_count: usize,
        columns: usize,
        row_extent: f32,
        viewport_extent: f32,
        overscan_rows: usize,
    ) -> Result<Self, UiValidationError> {
        if columns == 0 || columns > 256 {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_VIRTUAL_GRID_COLUMNS",
                "virtual grid columns must be within 1..=256",
            ));
        }
        let rows = item_count.div_ceil(columns);
        Ok(Self {
            list: VirtualListState::new(rows, row_extent, viewport_extent, overscan_rows)?,
            item_count,
            columns,
        })
    }

    pub fn set_scroll_offset(&mut self, offset: f32) -> Result<(), UiValidationError> {
        self.list.set_scroll_offset(offset)
    }

    pub fn configure(
        &mut self,
        item_count: usize,
        columns: usize,
        viewport_extent: f32,
    ) -> Result<(), UiValidationError> {
        if columns == 0 || columns > 256 {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_VIRTUAL_GRID_COLUMNS",
                "virtual grid columns must be within 1..=256",
            ));
        }
        self.item_count = item_count;
        self.columns = columns;
        self.list.set_item_count(item_count.div_ceil(columns))?;
        self.list.set_viewport_extent(viewport_extent)
    }

    pub fn scroll_by(&mut self, delta: f32) -> Result<(), UiValidationError> {
        self.list.scroll_by(delta)
    }

    pub fn visible_items(&mut self) -> VisibleRange {
        let rows = self.list.visible_range();
        VisibleRange {
            start: rows.start.saturating_mul(self.columns).min(self.item_count),
            end: rows.end.saturating_mul(self.columns).min(self.item_count),
        }
    }

    pub fn instantiated_count(&mut self) -> usize {
        self.visible_items().len()
    }

    pub fn visible_leading_extent(&mut self, range: VisibleRange) -> f32 {
        let row = range.start / self.columns;
        self.list
            .item_offset(row)
            .map_or(0.0, |offset| (offset - self.list.scroll_offset).max(0.0))
    }
}

#[derive(Debug)]
struct LruEntry<V> {
    value: V,
    bytes: usize,
    last_used: u64,
}

#[derive(Debug)]
pub struct BoundedLru<K, V> {
    entries: BTreeMap<K, LruEntry<V>>,
    max_entries: usize,
    max_bytes: usize,
    current_bytes: usize,
    clock: u64,
}

impl<K: Ord + Clone, V> BoundedLru<K, V> {
    pub fn new(max_entries: usize, max_bytes: usize) -> Result<Self, UiValidationError> {
        if max_entries == 0 || max_bytes == 0 {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_LRU_LIMIT",
                "LRU entry and byte limits must be positive",
            ));
        }
        Ok(Self {
            entries: BTreeMap::new(),
            max_entries,
            max_bytes,
            current_bytes: 0,
            clock: 0,
        })
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.clock = self.clock.saturating_add(1);
        let entry = self.entries.get_mut(key)?;
        entry.last_used = self.clock;
        Some(&entry.value)
    }

    pub fn insert(
        &mut self,
        key: K,
        value: V,
        bytes: usize,
    ) -> Result<Vec<(K, V)>, UiValidationError> {
        if bytes == 0 || bytes > self.max_bytes {
            return Err(UiValidationError::invalid(
                "ASTRA_UI_LRU_ENTRY_SIZE",
                "LRU entry byte size must be positive and fit the cache",
            ));
        }
        self.clock = self.clock.saturating_add(1);
        if let Some(previous) = self.entries.remove(&key) {
            self.current_bytes = self.current_bytes.saturating_sub(previous.bytes);
        }
        self.entries.insert(
            key,
            LruEntry {
                value,
                bytes,
                last_used: self.clock,
            },
        );
        self.current_bytes = self.current_bytes.saturating_add(bytes);
        let mut evicted = Vec::new();
        while self.entries.len() > self.max_entries || self.current_bytes > self.max_bytes {
            let key = self
                .entries
                .iter()
                .min_by_key(|(key, entry)| (entry.last_used, *key))
                .map(|(key, _)| key.clone())
                .expect("cache contains an entry while over budget");
            let entry = self
                .entries
                .remove(&key)
                .expect("selected LRU entry still exists");
            self.current_bytes = self.current_bytes.saturating_sub(entry.bytes);
            evicted.push((key, entry.value));
        }
        Ok(evicted)
    }

    pub fn current_bytes(&self) -> usize {
        self.current_bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn validate_extent(field: &str, extent: f32) -> Result<(), UiValidationError> {
    if !extent.is_finite() || extent <= 0.0 {
        return Err(UiValidationError::invalid(
            "ASTRA_UI_VIRTUAL_EXTENT",
            format!("{field} must be finite and positive"),
        ));
    }
    Ok(())
}
