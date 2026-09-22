use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{
    compile_astra_project, AstraSource, AstraSourceRole, CompileAstraProjectOptions,
    CompiledVnProject, EditBatch, EditError, VnError,
};

const MAX_HISTORY: usize = 100;
const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentSnapshot {
    pub path: String,
    pub version: u64,
    pub text: String,
}

#[derive(Debug, Clone)]
struct Document {
    snapshot: DocumentSnapshot,
    role: AstraSourceRole,
    saved_text: String,
}

/// One transaction owner for text, graph, timeline and external editing tools.
#[derive(Debug, Default)]
pub struct AuthoringWorkspace {
    documents: BTreeMap<String, Document>,
    undo: VecDeque<Vec<DocumentSnapshot>>,
    redo: VecDeque<Vec<DocumentSnapshot>>,
}

impl AuthoringWorkspace {
    pub fn open(&mut self, source: AstraSource) -> Result<(), EditError> {
        validate_path(&source.path)?;
        if source.text.len() > MAX_DOCUMENT_BYTES {
            return Err(EditError::Limit);
        }
        if self.documents.contains_key(&source.path) {
            return Err(EditError::AlreadyOpen);
        }
        self.documents.insert(
            source.path.clone(),
            Document {
                snapshot: DocumentSnapshot {
                    path: source.path,
                    version: 1,
                    text: source.text.clone(),
                },
                role: source.role,
                saved_text: source.text,
            },
        );
        // History cannot cross a change in the set of open documents.
        self.undo.clear();
        self.redo.clear();
        Ok(())
    }

    pub fn document(&self, path: &str) -> Result<&DocumentSnapshot, EditError> {
        self.documents
            .get(path)
            .map(|d| &d.snapshot)
            .ok_or(EditError::NotOpen)
    }

    pub fn documents(&self) -> impl Iterator<Item = &DocumentSnapshot> {
        self.documents.values().map(|d| &d.snapshot)
    }

    pub fn is_dirty(&self, path: &str) -> Result<bool, EditError> {
        let d = self.documents.get(path).ok_or(EditError::NotOpen)?;
        Ok(d.saved_text != d.snapshot.text)
    }

    /// Call only after the exact snapshot has been successfully persisted.
    pub fn mark_saved(&mut self, path: &str, version: u64) -> Result<(), EditError> {
        let d = self.documents.get_mut(path).ok_or(EditError::NotOpen)?;
        if d.snapshot.version != version {
            return Err(EditError::Conflict);
        }
        d.saved_text.clone_from(&d.snapshot.text);
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

    /// Validate every document and range before changing any document.
    pub fn apply(&mut self, batch: EditBatch) -> Result<(), EditError> {
        if batch.documents.is_empty() {
            return Err(EditError::EmptyBatch);
        }
        let mut changed = BTreeMap::new();
        let mut before = Vec::new();
        for edit in batch.documents {
            if changed.contains_key(&edit.path) {
                return Err(EditError::DuplicateDocument);
            }
            let old = self.document(&edit.path)?;
            if old.version != edit.version {
                return Err(EditError::Conflict);
            }
            let version = old.version.checked_add(1).ok_or(EditError::Limit)?;
            let text = edit.apply_to(&old.text)?;
            if text.len() > MAX_DOCUMENT_BYTES {
                return Err(EditError::Limit);
            }
            before.push(old.clone());
            changed.insert(
                edit.path.clone(),
                DocumentSnapshot {
                    path: edit.path,
                    version,
                    text,
                },
            );
        }
        for (path, snapshot) in changed {
            self.documents
                .get_mut(&path)
                .expect("validated document")
                .snapshot = snapshot;
        }
        push_history(&mut self.undo, before);
        self.redo.clear();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), EditError> {
        let before = self.undo.back().ok_or(EditError::NoHistory)?.clone();
        let after = self.restore(&before)?;
        self.undo.pop_back();
        push_history(&mut self.redo, after);
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), EditError> {
        let after = self.redo.back().ok_or(EditError::NoHistory)?.clone();
        let before = self.restore(&after)?;
        self.redo.pop_back();
        push_history(&mut self.undo, before);
        Ok(())
    }

    fn restore(&mut self, values: &[DocumentSnapshot]) -> Result<Vec<DocumentSnapshot>, EditError> {
        let mut previous = Vec::new();
        for value in values {
            let current = self.document(&value.path)?;
            current.version.checked_add(1).ok_or(EditError::Limit)?;
            previous.push(current.clone());
        }
        for value in values {
            let current = &mut self
                .documents
                .get_mut(&value.path)
                .expect("validated document")
                .snapshot;
            current.version += 1;
            current.text.clone_from(&value.text);
        }
        Ok(previous)
    }
}

fn push_history(history: &mut VecDeque<Vec<DocumentSnapshot>>, value: Vec<DocumentSnapshot>) {
    if history.len() == MAX_HISTORY {
        history.pop_front();
    }
    history.push_back(value);
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
