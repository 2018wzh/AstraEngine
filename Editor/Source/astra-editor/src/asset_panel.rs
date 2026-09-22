use super::*;
use astra_editor::asset_import::{prepare_image, ImageImport};

pub(super) struct ImportForm {
    token: uuid::Uuid,
    source: PathBuf,
    destination: Entity<InputState>,
    asset_id: Entity<InputState>,
    license: Entity<InputState>,
    root: Entity<InputState>,
    busy: bool,
}
impl Editor {
    fn choose_asset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.asset_roots.is_empty() || self.project.profiles.is_empty() {
            self.status =
                "Open a project with asset_roots and cook profiles before importing".into();
            cx.notify();
            return;
        }
        let token = uuid::Uuid::new_v4();
        self.asset_dialog = Some(token);
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import image".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = chosen.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.asset_dialog != Some(token) {
                    return;
                }
                this.asset_dialog = None;
                let path = match result {
                    Ok(Ok(Some(paths))) => paths.into_iter().next(),
                    Ok(Err(error)) => {
                        this.status = error.to_string();
                        cx.notify();
                        return;
                    }
                    _ => None,
                };
                let Some(path) = path else {
                    return;
                };
                let filename = path.file_name().unwrap_or_default().to_string_lossy();
                let stem = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || c == '-' {
                            c.to_ascii_lowercase()
                        } else {
                            '_'
                        }
                    })
                    .collect::<String>();
                let mut input = |value: String| {
                    cx.new(|cx| {
                        let mut state = InputState::new(window, cx);
                        state.set_value(value, window, cx);
                        state
                    })
                };
                let root = if this.project.asset_roots.len() == 1 {
                    this.project.asset_roots[0].clone()
                } else {
                    String::new()
                };
                this.asset_import = Some(ImportForm {
                    token: uuid::Uuid::new_v4(),
                    destination: input(format!("Images/{filename}")),
                    asset_id: input(format!("asset:/imported/{stem}")),
                    license: input(String::new()),
                    root: input(root),
                    source: path,
                    busy: false,
                });
                cx.notify();
            });
        })
        .detach();
    }
    fn import_asset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.asset_import.as_mut() else {
            return;
        };
        if form.busy {
            return;
        }
        let settings = ImageImport {
            destination: form.destination.read(cx).value().to_string(),
            asset_id: form.asset_id.read(cx).value().to_string(),
            license: form.license.read(cx).value().to_string(),
            sidecar_root: form.root.read(cx).value().to_string(),
            profiles: self.project.profiles.clone(),
        };
        let token = form.token;
        let source = form.source.clone();
        form.busy = true;
        let prepared = cx
            .background_executor()
            .spawn(async move { prepare_image(&source, settings) });
        cx.spawn_in(window, async move |this, cx| {
            let result = prepared.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if !this
                    .asset_import
                    .as_ref()
                    .is_some_and(|form| form.token == token)
                {
                    return;
                }
                match result.and_then(|image| this.project.import_image(image)) {
                    Ok(path) => {
                        this.cancel_agent();
                        this.preview = None;
                        this.asset_import = None;
                        this.status = format!("Imported {path}");
                    }
                    Err(error) => {
                        this.asset_import.as_mut().unwrap().busy = false;
                        this.status = error.to_string();
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn asset_import_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = div().flex().flex_col().gap_1().child(
            Button::new("choose-asset")
                .label("Import image")
                .on_click(cx.listener(|this, _, window, cx| this.choose_asset(window, cx))),
        );
        if let Some(form) = &self.asset_import {
            panel =
                panel
                    .child(format!(
                        "Import {}",
                        form.source
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                    ))
                    .child("Project-relative image path")
                    .child(Input::new(&form.destination).disabled(form.busy))
                    .child("Asset ID")
                    .child(Input::new(&form.asset_id).disabled(form.busy))
                    .child("License")
                    .child(Input::new(&form.license).disabled(form.busy))
                    .child(format!(
                        "Sidecar root: {}",
                        self.project.asset_roots.join(", ")
                    ))
                    .child(Input::new(&form.root).disabled(form.busy))
                    .child(format!(
                        "Cook profiles: {}",
                        self.project.profiles.join(", ")
                    ))
                    .child("Existing names are not overwritten. Change the path / ID or cancel.")
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("import-image")
                                    .label(if form.busy { "Importing…" } else { "Import" })
                                    .disabled(form.busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.import_asset(window, cx)
                                    })),
                            )
                            .child(Button::new("cancel-import").label("Cancel").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.asset_import = None;
                                    this.asset_dialog = None;
                                    cx.notify();
                                }),
                            )),
                    );
        }
        panel.into_any_element()
    }
}
