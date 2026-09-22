use astra_vn_editor::{AuthoringWorkspace, EditBatch};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    Autonomous,
    ReviewEachBatch,
}

/// Session generation is invalidated before cancelling transport work.
pub struct AgentEdits {
    generation: u64,
    active: bool,
    mode: EditMode,
    pending: Option<EditBatch>,
}

impl Default for AgentEdits {
    fn default() -> Self {
        Self {
            generation: 0,
            active: false,
            mode: EditMode::ReviewEachBatch,
            pending: None,
        }
    }
}

impl AgentEdits {
    pub fn begin(&mut self, mode: EditMode) -> anyhow::Result<u64> {
        anyhow::ensure!(!self.active, "An agent turn is already active");
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Session limit reached"))?;
        self.mode = mode;
        self.active = true;
        Ok(self.generation)
    }

    pub fn cancel(&mut self) {
        self.active = false;
        self.pending = None;
    }

    pub fn pending(&self) -> Option<&EditBatch> {
        self.pending.as_ref()
    }

    pub fn submit(
        &mut self,
        generation: u64,
        batch: EditBatch,
        documents: &mut AuthoringWorkspace,
    ) -> anyhow::Result<bool> {
        anyhow::ensure!(
            self.active && generation == self.generation,
            "Cancelled or stale agent turn"
        );
        anyhow::ensure!(self.pending.is_none(), "Review the current batch first");
        for edit in &batch.documents {
            anyhow::ensure!(
                documents.document(&edit.path)?.version == edit.version,
                "Document version conflict"
            );
        }
        match self.mode {
            EditMode::Autonomous => {
                documents.apply(batch)?;
                Ok(true)
            }
            EditMode::ReviewEachBatch => {
                self.pending = Some(batch);
                Ok(false)
            }
        }
    }

    pub fn resolve(
        &mut self,
        approve: bool,
        documents: &mut AuthoringWorkspace,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.active, "Agent turn was cancelled");
        let batch = self
            .pending
            .take()
            .ok_or_else(|| anyhow::anyhow!("No batch to review"))?;
        if approve {
            documents.apply(batch)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_vn_editor::{AstraSource, DocumentEdits, TextEdit};

    #[test]
    fn cancel_and_manual_conflict_cannot_apply_late_results() {
        let mut documents = AuthoringWorkspace::default();
        documents
            .open(AstraSource::story("main.astra", "old"))
            .unwrap();
        let batch = EditBatch {
            documents: vec![DocumentEdits {
                path: "main.astra".into(),
                version: 1,
                edits: vec![TextEdit {
                    start: 0,
                    end: 3,
                    replacement: "new".into(),
                }],
            }],
        };
        let mut agent = AgentEdits::default();
        let generation = agent.begin(EditMode::ReviewEachBatch).unwrap();
        assert!(!agent
            .submit(generation, batch.clone(), &mut documents)
            .unwrap());
        documents.apply(batch.clone()).unwrap();
        assert!(agent.resolve(true, &mut documents).is_err());
        agent.cancel();
        assert!(agent
            .submit(generation, batch.clone(), &mut documents)
            .is_err());
        agent.begin(EditMode::Autonomous).unwrap();
        assert!(agent.submit(generation, batch, &mut documents).is_err());
    }
}
