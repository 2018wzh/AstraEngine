use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use astra_vn_editor::{AstraSource, AuthoringWorkspace, DocumentEdits, EditBatch, TextEdit};

#[derive(Clone, PartialEq, Eq)]
pub struct ProjectRevision {
    session: uuid::Uuid,
    documents: BTreeMap<String, u64>,
}

pub struct Project {
    session: uuid::Uuid,
    pub documents: AuthoringWorkspace,
    pub active: String,
    pub(crate) root: PathBuf,
    pub asset_roots: Vec<String>,
    pub profiles: Vec<String>,
    disk_text: BTreeMap<String, String>,
    pub content: Vec<String>,
    options: astra_vn_editor::CompileAstraProjectOptions,
}

impl Project {
    pub fn session_id(&self) -> uuid::Uuid {
        self.session
    }

    /// Capture the exact source revisions a destructive confirmation refers to.
    pub fn revision(&self) -> ProjectRevision {
        ProjectRevision {
            session: self.session,
            documents: self
                .documents
                .documents()
                .map(|d| (d.path.clone(), d.version))
                .collect(),
        }
    }

    pub fn layout_path(&self) -> PathBuf {
        self.root.join(".astra-cache/editor-layout.json")
    }
    pub fn source_path(&self) -> PathBuf {
        self.root.join(&self.active)
    }
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let path = path.canonicalize()?;
        if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("yaml" | "yml")
        ) {
            let files = crate::project_files::load(&path)?;
            let active = files.sources[0].path.clone();
            let mut documents = AuthoringWorkspace::default();
            let mut disk_text = BTreeMap::new();
            for source in files.sources {
                disk_text.insert(source.path.clone(), source.text.clone());
                documents.open(source)?;
            }
            return Ok(Self {
                session: uuid::Uuid::new_v4(),
                documents,
                active,
                root: path.parent().unwrap().to_path_buf(),
                disk_text,
                content: files.content,
                asset_roots: files.asset_roots,
                profiles: files.profiles,
                options: files.options,
            });
        }
        let active = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid source filename"))?
            .to_string();
        let text = std::fs::read_to_string(&path)?;
        let mut documents = AuthoringWorkspace::default();
        documents.open(AstraSource::story(&active, &text))?;
        Ok(Self {
            session: uuid::Uuid::new_v4(),
            documents,
            active: active.clone(),
            root: path.parent().unwrap().to_path_buf(),
            disk_text: BTreeMap::from([(active.clone(), text)]),
            content: Vec::new(),
            asset_roots: Vec::new(),
            profiles: Vec::new(),
            options: Default::default(),
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
        let path = self.source_path();
        anyhow::ensure!(
            std::fs::read_to_string(&path)? == self.disk_text[&self.active],
            "Source changed on disk; reopen before saving"
        );
        let snapshot = self.documents.document(&self.active)?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(path.parent().expect("absolute source parent"))?;
        use std::io::Write;
        temporary.write_all(snapshot.text.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary.persist(&path)?;
        self.disk_text
            .insert(self.active.clone(), snapshot.text.clone());
        let version = snapshot.version;
        self.documents.mark_saved(&self.active, version)?;
        Ok(())
    }

    pub fn compile(&self) -> Result<astra_vn_editor::CompiledVnProject, astra_vn_editor::VnError> {
        self.documents.compile(self.options.clone())
    }

    pub fn any_dirty(&self) -> bool {
        self.documents
            .documents()
            .any(|d| self.documents.is_dirty(&d.path).unwrap_or(true))
    }

    pub fn activate(&mut self, path: &str) -> anyhow::Result<()> {
        self.documents.document(path)?;
        self.active = path.to_string();
        Ok(())
    }

    pub fn save_all(&mut self) -> anyhow::Result<()> {
        // Check every source before starting writes; report partial IO failures explicitly.
        for (path, expected) in &self.disk_text {
            anyhow::ensure!(
                std::fs::read_to_string(self.root.join(path))? == *expected,
                "Source changed on disk; reopen before saving"
            );
        }
        let active = self.active.clone();
        let paths = self
            .documents
            .documents()
            .map(|d| d.path.clone())
            .collect::<Vec<_>>();
        let result = paths.into_iter().try_for_each(|path| {
            self.active = path;
            self.save()
        });
        self.active = active;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_revision_rejects_reopen_and_edit_undo() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("main.astra");
        std::fs::write(&path, "# original\n").unwrap();
        let mut project = Project::open(&path).unwrap();
        let confirmation = project.revision();
        assert!(project.revision() == confirmation);
        project.save_all().unwrap();
        assert!(project.revision() == confirmation);
        project.replace("# changed\n".into()).unwrap();
        assert!(project.revision() != confirmation);
        project.documents.undo().unwrap();
        assert!(!project.any_dirty());
        assert!(project.revision() != confirmation);
        let reopened = Project::open(&path).unwrap();
        assert!(reopened.revision() != confirmation);
        assert_ne!(reopened.session_id(), project.session_id());
    }

    #[test]
    fn project_switch_keeps_unsaved_documents_and_batch_undo() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        std::fs::create_dir(root.join("Scripts")).unwrap();
        std::fs::write(
            root.join("project.yaml"),
            "nativevn:\n  sources: [Scripts]\n",
        )
        .unwrap();
        std::fs::write(root.join("Scripts/a.astra"), "# first\n").unwrap();
        std::fs::write(root.join("Scripts/b.astra"), "# second\n").unwrap();
        let mut project = Project::open(&root.join("project.yaml")).unwrap();
        project.replace("# changed\n".into()).unwrap();
        project.activate("Scripts/b.astra").unwrap();
        assert!(project.any_dirty());
        project.documents.undo().unwrap();
        assert!(!project.any_dirty());
        project.documents.redo().unwrap();
        project.save_all().unwrap();
        assert!(!project.any_dirty());
        assert_eq!(
            std::fs::read_to_string(root.join("Scripts/a.astra")).unwrap(),
            "# changed\n"
        );
    }

    #[test]
    fn project_rejects_source_roots_outside_project() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("project.yaml");
        std::fs::write(&path, "nativevn:\n  sources: [../outside]\n").unwrap();
        assert!(Project::open(&path).is_err());
    }

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
