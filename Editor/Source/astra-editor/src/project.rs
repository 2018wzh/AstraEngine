use std::path::{Path, PathBuf};

use astra_vn_editor::{AstraSource, AuthoringWorkspace, DocumentEdits, EditBatch, TextEdit};

pub struct Project {
    pub documents: AuthoringWorkspace,
    pub active: String,
    path: PathBuf,
    disk_text: String,
}

impl Project {
    pub fn source_path(&self) -> &Path {
        &self.path
    }
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let path = path.canonicalize()?;
        let active = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid source filename"))?
            .to_string();
        let text = std::fs::read_to_string(&path)?;
        let mut documents = AuthoringWorkspace::default();
        documents.open(AstraSource::story(&active, &text))?;
        Ok(Self {
            documents,
            active,
            path,
            disk_text: text,
        })
    }

    pub fn replace(&mut self, text: String) -> anyhow::Result<()> {
        let snapshot = self.documents.document(&self.active)?;
        if snapshot.text == text {
            return Ok(());
        }
        self.documents.apply(EditBatch {
            documents: vec![DocumentEdits {
                path: self.active.clone(),
                version: snapshot.version,
                edits: vec![TextEdit {
                    start: 0,
                    end: snapshot.text.len(),
                    replacement: text,
                }],
            }],
        })?;
        Ok(())
    }

    pub fn save(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            std::fs::read_to_string(&self.path)? == self.disk_text,
            "Source changed on disk; reopen before saving"
        );
        let snapshot = self.documents.document(&self.active)?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(self.path.parent().expect("absolute source parent"))?;
        use std::io::Write;
        temporary.write_all(snapshot.text.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path)?;
        self.disk_text.clone_from(&snapshot.text);
        let version = snapshot.version;
        self.documents.mark_saved(&self.active, version)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_reopen_and_external_write_conflict() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("main.astra");
        std::fs::write(&path, "# first\n").unwrap();
        let mut project = Project::open(&path).unwrap();
        project.replace("# second\n".into()).unwrap();
        project.save().unwrap();
        assert!(!project.documents.is_dirty("main.astra").unwrap());
        assert_eq!(
            Project::open(&path)
                .unwrap()
                .documents
                .document("main.astra")
                .unwrap()
                .text,
            "# second\n"
        );
        project.replace("# third\n".into()).unwrap();
        std::fs::write(&path, "# external\n").unwrap();
        assert!(project.save().is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "# external\n");
    }
}
