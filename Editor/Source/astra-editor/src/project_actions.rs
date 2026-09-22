use super::*;

impl Editor {
    pub fn choose_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_dialog {
            return;
        }
        self.project_dialog = true;
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open NativeVN project.yaml or Astra source".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let path = match selected.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Err(error)) => {
                    let _ = this.update_in(cx, |this, _, cx| {
                        this.project_dialog = false;
                        this.status = error.to_string();
                        cx.notify();
                    });
                    return;
                }
                _ => None,
            };
            let Some(path) = path else {
                let _ = this.update_in(cx, |this, _, cx| {
                    this.project_dialog = false;
                    cx.notify();
                });
                return;
            };
            let confirmation = this.update_in(cx, |this, window, cx| {
                let revision = this.project.revision();
                let confirmation = this.project.any_dirty().then(|| {
                    window.prompt(
                        PromptLevel::Warning,
                        "Unsaved source changes",
                        Some("Save or discard changes before opening the project."),
                        &["Cancel", "Save and open", "Discard and open"],
                        cx,
                    )
                });
                (revision, confirmation)
            });
            let Ok((revision, confirmation)) = confirmation else {
                return;
            };
            let choice = match confirmation {
                Some(confirmation) => confirmation.await.unwrap_or(0),
                None => 2,
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.project_dialog = false;
                if choice == 0 {
                    cx.notify();
                    return;
                }
                if this.project.revision() != revision {
                    this.status =
                        "Project changed while confirming; open again to review current changes"
                            .into();
                    cx.notify();
                    return;
                }
                if choice == 1 {
                    if let Err(error) = this.project.save_all() {
                        this.status = error.to_string();
                        cx.notify();
                        return;
                    }
                }
                match Project::open(&path) {
                    Ok(project) => {
                        this.cancel_agent();
                        this.preview = None;
                        // A new project must bind its own packaging target/tools explicitly.
                        this.preview_config = None;
                        this.asset_import = None;
                        this.asset_dialog = None;
                        this.preview_dialog = None;
                        this.project = project;
                        this.panels = panels::WorkspacePanels::new(window, cx);
                        this.sync(window, cx);
                    }
                    Err(error) => {
                        this.status = error.to_string();
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub fn choose_preview_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = uuid::Uuid::new_v4();
        self.preview_dialog = Some(token);
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select preview configuration JSON".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = match selected.await {
                Ok(Ok(Some(paths))) => paths.first().map(|path| -> anyhow::Result<PreviewConfig> {
                    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
                }),
                Ok(Err(error)) => Some(Err(error)),
                _ => None,
            };
            let _ = this.update_in(cx, |this, _, cx| {
                if this.preview_dialog != Some(token) {
                    return;
                }
                this.preview_dialog = None;
                if let Some(result) = result {
                    match result {
                        Ok(config) => {
                            this.preview = None;
                            this.preview_config = Some(config);
                            this.status = "Preview configuration loaded".into();
                        }
                        Err(error) => this.status = error.to_string(),
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
