use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{
    compile_astra_project, AstraSource, AstraSourceRole, CompileAstraProjectOptions,
    CompiledVnProject, EditBatch, EditError, VnError,
};

const MAX_HISTORY: usize = 100;
pub(crate) const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_HISTORY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentSnapshot {
    pub path: String,
    pub version: u64,
    pub text: String,
}

#[derive(Debug, Clone)]
struct Document {
    snapshot: Arc<DocumentSnapshot>,
    role: AstraSourceRole,
    saved_snapshot: Arc<DocumentSnapshot>,
}

/// One transaction owner for text, graph, timeline and external editing tools.
#[derive(Debug)]
pub struct AuthoringWorkspace {
    documents: BTreeMap<String, Document>,
    undo: VecDeque<HistoryEntry>,
    redo: VecDeque<HistoryEntry>,
    history_budget: usize,
    history_reserved_bytes: usize,
}

#[derive(Debug)]
struct HistoryDocument {
    path: String,
    edits: Vec<crate::TextEdit>,
}

#[derive(Debug)]
struct HistoryEntry {
    documents: Vec<HistoryDocument>,
    reserved_bytes: usize,
}

type DocumentChanges = BTreeMap<String, Arc<DocumentSnapshot>>;

impl Default for AuthoringWorkspace {
    fn default() -> Self {
        Self {
            documents: BTreeMap::new(),
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            history_budget: DEFAULT_HISTORY_BYTES,
            history_reserved_bytes: 0,
        }
    }
}

/// Immutable source versions shared with a compiler without copying document text.
#[derive(Debug, Clone)]
pub struct CompilationSnapshot {
    sources: Vec<(AstraSourceRole, Arc<DocumentSnapshot>)>,
}

impl CompilationSnapshot {
    pub fn compile(
        &self,
        options: CompileAstraProjectOptions,
    ) -> Result<CompiledVnProject, VnError> {
        compile_astra_project(
            self.sources.iter().map(|(role, document)| AstraSource {
                path: document.path.clone(),
                text: document.text.clone(),
                role: *role,
            }),
            options,
        )
    }

    pub(crate) fn same_revision(&self, other: &Self) -> bool {
        self.sources.len() == other.sources.len()
            && self.sources.iter().zip(&other.sources).all(
                |((role, source), (other_role, other_source))| {
                    role == other_role && Arc::ptr_eq(source, other_source)
                },
            )
    }
}

impl AuthoringWorkspace {
    pub fn with_history_budget(bytes: usize) -> Result<Self, EditError> {
        if bytes == 0 {
            return Err(EditError::HistoryLimit);
        }
        Ok(Self {
            history_budget: bytes,
            ..Self::default()
        })
    }

    /// Conservative reservation for either undo or redo, excluding live documents.
    pub fn history_reserved_bytes(&self) -> usize {
        self.history_reserved_bytes
    }

    pub fn open(&mut self, source: AstraSource) -> Result<(), EditError> {
        validate_path(&source.path)?;
        if source.text.len() > MAX_DOCUMENT_BYTES {
            return Err(EditError::Limit);
        }
        if self.documents.contains_key(&source.path) {
            return Err(EditError::AlreadyOpen);
        }
        let snapshot = Arc::new(DocumentSnapshot {
            path: source.path,
            version: 1,
            text: source.text,
        });
        self.documents.insert(
            snapshot.path.clone(),
            Document {
                saved_snapshot: Arc::clone(&snapshot),
                snapshot,
                role: source.role,
            },
        );
        // History cannot cross a change in the set of open documents.
        self.undo.clear();
        self.redo.clear();
        self.history_reserved_bytes = 0;
        Ok(())
    }

    pub fn document(&self, path: &str) -> Result<&DocumentSnapshot, EditError> {
        self.documents
            .get(path)
            .map(|d| d.snapshot.as_ref())
            .ok_or(EditError::NotOpen)
    }

    pub fn documents(&self) -> impl Iterator<Item = &DocumentSnapshot> {
        self.documents.values().map(|d| d.snapshot.as_ref())
    }

    pub fn compilation_snapshot(&self) -> CompilationSnapshot {
        CompilationSnapshot {
            sources: self
                .documents
                .values()
                .map(|d| (d.role, Arc::clone(&d.snapshot)))
                .collect(),
        }
    }

    pub fn is_dirty(&self, path: &str) -> Result<bool, EditError> {
        let d = self.documents.get(path).ok_or(EditError::NotOpen)?;
        Ok(
            !Arc::ptr_eq(&d.saved_snapshot, &d.snapshot)
                && d.saved_snapshot.text != d.snapshot.text,
        )
    }

    /// Call only after the exact snapshot has been successfully persisted.
    pub fn mark_saved(&mut self, path: &str, version: u64) -> Result<(), EditError> {
        let d = self.documents.get_mut(path).ok_or(EditError::NotOpen)?;
        if d.snapshot.version != version {
            return Err(EditError::Conflict);
        }
        d.saved_snapshot = Arc::clone(&d.snapshot);
        Ok(())
    }

    pub fn compile(
        &self,
        options: CompileAstraProjectOptions,
    ) -> Result<CompiledVnProject, VnError> {
        compile_astra_project(
            self.documents.values().map(|d| AstraSource {
                path: d.snapshot.path.clone(),
                text: d.snapshot.text.clone(),
                role: d.role,
            }),
            options,
        )
    }

    /// Validate the complete transaction, including its undo reservation, before
    /// changing any document or evicting existing history.
    pub fn apply(&mut self, batch: EditBatch) -> Result<(), EditError> {
        if batch.documents.is_empty() {
            return Err(EditError::EmptyBatch);
        }
        let mut changed = DocumentChanges::new();
        let mut inverse = HistoryEntry {
            documents: Vec::new(),
            reserved_bytes: 2 * std::mem::size_of::<HistoryEntry>(),
        };
        for edit in batch.documents {
            let path = edit.path;
            if changed.contains_key(&path) {
                return Err(EditError::DuplicateDocument);
            }
            let old = self.document(&path)?;
            if old.version != edit.version {
                return Err(EditError::Conflict);
            }
            let version = old.version.checked_add(1).ok_or(EditError::Limit)?;
            let metadata = path
                .len()
                .checked_add(4 * std::mem::size_of::<HistoryDocument>())
                .ok_or(EditError::HistoryLimit)?;
            let reserved = inverse
                .reserved_bytes
                .checked_add(metadata)
                .ok_or(EditError::HistoryLimit)?;
            let available = self
                .history_budget
                .checked_sub(reserved)
                .ok_or(EditError::HistoryLimit)?;
            let prepared =
                crate::history::prepare(&old.text, std::borrow::Cow::Owned(edit.edits), available)?;
            inverse.reserved_bytes = reserved + prepared.reserved_bytes;
            changed.insert(
                path.clone(),
                Arc::new(DocumentSnapshot {
                    path: path.clone(),
                    version,
                    text: prepared.text,
                }),
            );
            inverse.documents.push(HistoryDocument {
                path,
                edits: prepared.inverse,
            });
        }
        self.commit(changed);
        for entry in self.redo.drain(..) {
            self.history_reserved_bytes -= entry.reserved_bytes;
        }
        while self.undo.len() == MAX_HISTORY
            || self.history_reserved_bytes > self.history_budget - inverse.reserved_bytes
        {
            let retired = self
                .undo
                .pop_front()
                .expect("admitted history fits an empty queue");
            self.history_reserved_bytes -= retired.reserved_bytes;
        }
        self.history_reserved_bytes += inverse.reserved_bytes;
        self.undo.push_back(inverse);
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), EditError> {
        self.move_history(true)
    }

    pub fn redo(&mut self) -> Result<(), EditError> {
        self.move_history(false)
    }

    fn move_history(&mut self, undo: bool) -> Result<(), EditError> {
        let entry = if undo {
            self.undo.back()
        } else {
            self.redo.back()
        }
        .ok_or(EditError::NoHistory)?;
        let mut inverse = HistoryEntry {
            documents: Vec::new(),
            reserved_bytes: entry.reserved_bytes,
        };
        let mut changed = DocumentChanges::new();
        for document in &entry.documents {
            let current = self.document(&document.path)?;
            let version = current.version.checked_add(1).ok_or(EditError::Limit)?;
            let prepared = crate::history::prepare(
                &current.text,
                std::borrow::Cow::Borrowed(&document.edits),
                self.history_budget,
            )?;
            changed.insert(
                document.path.clone(),
                Arc::new(DocumentSnapshot {
                    path: document.path.clone(),
                    version,
                    text: prepared.text,
                }),
            );
            inverse.documents.push(HistoryDocument {
                path: document.path.clone(),
                edits: prepared.inverse,
            });
        }
        self.commit(changed);
        if undo {
            self.undo.pop_back();
            self.redo.push_back(inverse);
        } else {
            self.redo.pop_back();
            self.undo.push_back(inverse);
        }
        Ok(())
    }

    fn commit(&mut self, changed: DocumentChanges) {
        for (path, snapshot) in changed {
            self.documents
                .get_mut(&path)
                .expect("validated document")
                .snapshot = snapshot;
        }
    }
}

fn validate_path(path: &str) -> Result<(), EditError> {
    if path.is_empty()
        || !path.ends_with(".astra")
        || path.contains(['\\', ':', '\0'])
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(EditError::InvalidPath);
    }
    Ok(())
}
