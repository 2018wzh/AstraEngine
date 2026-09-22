use clap::Parser;
use std::path::PathBuf;

use astra_editor::preview::{Preview, PreviewConfig};
use astra_editor::project::Project;
use astra_editor::{
    agent::{AgentEdits, EditMode},
    bridge::{EditorBridge, Request},
};
use gpui::{div, prelude::*, *};
use gpui_component::{
    button::Button,
    input::{Input, InputEvent, InputState},
    Disableable, Root,
};
mod agent_actions;
mod asset_panel;
mod dock_panels;
mod graph_panel;
mod panels;
mod project_actions;
mod render;
mod timeline_panel;
use agent_client_protocol::schema::v1::{
    RequestPermissionOutcome, RequestPermissionRequest, SelectedPermissionOutcome,
};

struct PermissionReview {
    generation: u64,
    request: Box<RequestPermissionRequest>,
    reply: tokio::sync::oneshot::Sender<RequestPermissionOutcome>,
}

struct Editor {
    asset_import: Option<asset_panel::ImportForm>,
    project: Project,
    input: Entity<InputState>,
    status: String,
    syncing: bool,
    preview_config: Option<PreviewConfig>,
    preview: Option<Preview>,
    preview_generation: u64,
    close_confirmed: bool,
    project_dialog: bool,
    bridge: EditorBridge,
    requests: tokio::sync::mpsc::Receiver<Request>,
    agent: AgentEdits,
    mode: EditMode,
    pending_reply: Option<tokio::sync::oneshot::Sender<anyhow::Result<()>>>,
    permission: Option<PermissionReview>,
    agent_command: Option<String>,
    agent_prompt: Entity<InputState>,
    agent_output: String,
    runtime: tokio::runtime::Handle,
    agent_task: Option<tokio::task::JoinHandle<()>>,
    diagnostic_position: Option<gpui_component::input::Position>,
    diagnostic_source: Option<String>,
    panels: panels::WorkspacePanels,
    _subscription: Subscription,
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.bridge.cancel();
        if let Some(reply) = self.pending_reply.take() {
            let _ = reply.send(Err(anyhow::anyhow!("Editor closed")));
        }
        if let Some(task) = self.agent_task.take() {
            task.abort();
        }
    }
}

impl Editor {
    fn new(
        project: Project,
        preview_config: Option<PreviewConfig>,
        transport: (EditorBridge, tokio::sync::mpsc::Receiver<Request>),
        agent_command: Option<String>,
        runtime: tokio::runtime::Handle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (bridge, requests) = transport;
        let text = project
            .documents
            .document(&project.active)
            .unwrap()
            .text
            .clone();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("text")
                .line_number(true)
        });
        let agent_prompt = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Ask the external ACP agent to edit this source")
        });
        input.update(cx, |input, cx| input.set_value(text, window, cx));
        let subscription = cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) && !this.syncing {
                let text = input.read(cx).value().to_string();
                if let Err(error) = this.project.replace(text) {
                    this.status = error.to_string();
                } else {
                    this.cancel_agent();
                    this.preview = None;
                    this.diagnose();
                }
                cx.notify();
            }
        });
        let mut editor = Self {
            panels: panels::WorkspacePanels::new(window, cx),
            asset_import: None,
            project,
            input,
            status: String::new(),
            syncing: false,
            preview_config,
            preview: None,
            preview_generation: 0,
            close_confirmed: false,
            project_dialog: false,
            bridge,
            requests,
            agent: AgentEdits::default(),
            mode: EditMode::ReviewEachBatch,
            pending_reply: None,
            permission: None,
            agent_command,
            agent_prompt,
            agent_output: String::new(),
            runtime,
            agent_task: None,
            diagnostic_position: None,
            diagnostic_source: None,
            _subscription: subscription,
        };
        editor.agent.begin(editor.mode).expect("new agent turn");
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
            if this
                .update_in(cx, |this, window, cx| {
                    this.follow_source_cursor(window, cx);
                    while let Ok(request) = this.requests.try_recv() {
                        match request {
                            Request::Permission {
                                generation,
                                request,
                                reply,
                            } => {
                                if generation != this.bridge.generation()
                                    || this.permission.is_some()
                                    || reply.is_closed()
                                {
                                    let _ = reply.send(RequestPermissionOutcome::Cancelled);
                                } else {
                                    this.permission = Some(PermissionReview {
                                        generation,
                                        request,
                                        reply,
                                    });
                                    cx.notify();
                                }
                            }
                            Request::AgentMessage { generation, text } => {
                                if generation == this.bridge.generation()
                                    && this.agent_output.len() < 16_384
                                {
                                    for character in text.chars() {
                                        if this.agent_output.len() + character.len_utf8() > 16_384 {
                                            break;
                                        }
                                        this.agent_output.push(character);
                                    }
                                    cx.notify();
                                }
                            }
                            Request::Read { reply } => {
                                let _ = reply.send(
                                    this.project
                                        .documents
                                        .document(&this.project.active)
                                        .cloned()
                                        .map_err(Into::into),
                                );
                            }
                            Request::Apply {
                                generation,
                                batch,
                                reply,
                            } => {
                                if generation != this.bridge.generation() || reply.is_closed() {
                                    let _ = reply.send(Err(anyhow::anyhow!("Cancelled request")));
                                    continue;
                                }
                                match this.agent.submit(
                                    generation,
                                    batch,
                                    &mut this.project.documents,
                                ) {
                                    Ok(true) => {
                                        let _ = reply.send(Ok(()));
                                        this.sync(window, cx);
                                    }
                                    Ok(false) => {
                                        this.pending_reply = Some(reply);
                                        cx.notify();
                                    }
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                    }
                                }
                            }
                        }
                    }
                    if this.pending_reply.as_ref().is_some_and(|r| r.is_closed()) {
                        this.cancel_agent();
                        cx.notify();
                    }
                    if this
                        .permission
                        .as_ref()
                        .is_some_and(|permission| permission.reply.is_closed())
                    {
                        this.permission = None;
                        cx.notify();
                    }
                    if let Some(preview) = &mut this.preview {
                        if let Err(error) = preview.poll(&this.project.documents) {
                            this.status = error.to_string();
                            this.preview = None;
                        }
                        cx.notify();
                    }
                })
                .is_err()
            {
                break;
            }
        })
        .detach();
        editor.diagnose();
        editor
    }

    fn diagnose(&mut self) {
        self.diagnostic_position = None;
        self.diagnostic_source = None;
        self.status = match self.project.compile() {
            Ok(project) => format!(
                "Compiled · {} source locations",
                project.story.source_map.len()
            ),
            Err(astra_vn_editor::VnError::Diagnostic(error)) => {
                if let Some(source) = error.source {
                    self.diagnostic_source = Some(source.source.clone());
                    self.diagnostic_position = Some(gpui_component::input::Position::new(
                        source.line.saturating_sub(1),
                        source.column.saturating_sub(1),
                    ));
                    format!(
                        "{}:{}:{} · {}: {}",
                        source.source, source.line, source.column, error.code, error.message
                    )
                } else {
                    format!("{}: {}", error.code, error.message)
                }
            }
            Err(error) => error.to_string(),
        };
    }

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview = None;
        self.syncing = true;
        let text = self
            .project
            .documents
            .document(&self.project.active)
            .unwrap()
            .text
            .clone();
        self.input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.syncing = false;
        self.diagnose();
        cx.notify();
    }
}

#[derive(Parser)]
struct Args {
    source: PathBuf,
    #[arg(long)]
    preview_config: Option<PathBuf>,
    #[arg(long)]
    agent: Option<String>,
    #[arg(long)]
    mcp: bool,
    #[arg(long)]
    check_project: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let project = Project::open(&args.source)?;
    if args.check_project {
        let compiled = project.compile()?;
        println!(
            "Compiled {} source documents, {} source locations",
            project.documents.documents().count(),
            compiled.story.source_map.len()
        );
        return Ok(());
    }
    let preview_config = args
        .preview_config
        .map(|path| -> anyhow::Result<PreviewConfig> {
            Ok(serde_json::from_slice(&std::fs::read(path)?)?)
        })
        .transpose()?;
    let (bridge, requests) = EditorBridge::new();
    let runtime = tokio::runtime::Runtime::new()?;
    if args.mcp {
        let bridge = bridge.clone();
        runtime.spawn(async move {
            if let Err(error) = astra_editor::mcp::serve(bridge).await {
                eprintln!("MCP: {error}");
            }
        });
    }
    let runtime_handle = runtime.handle().clone();
    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        let bounds = Bounds::centered(None, size(px(1100.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("AstraEditor");
                let editor = cx.new(|cx| {
                    Editor::new(
                        project,
                        preview_config,
                        (bridge, requests),
                        args.agent,
                        runtime_handle,
                        window,
                        cx,
                    )
                });
                let weak = editor.downgrade();
                window.on_window_should_close(cx, move |window, cx| {
                    weak.update(cx, |editor, cx| {
                        if editor.close_confirmed || !editor.project.any_dirty() {
                            return true;
                        }
                        let answer = window.prompt(
                            PromptLevel::Warning,
                            "Unsaved source changes",
                            Some("Save or discard changes before closing."),
                            &["Cancel", "Save and close", "Discard"],
                            cx,
                        );
                        cx.spawn_in(window, async move |this, cx| {
                            let choice = answer.await.unwrap_or(0);
                            if choice == 0 {
                                return;
                            }
                            let _ = this.update_in(cx, |this, window, cx| {
                                if choice == 1 {
                                    if let Err(error) = this.project.save_all() {
                                        this.status = error.to_string();
                                        cx.notify();
                                        return;
                                    }
                                }
                                this.close_confirmed = true;
                                this.preview = None;
                                window.remove_window();
                            });
                        })
                        .detach();
                        false
                    })
                    .unwrap_or(true)
                });
                cx.new(|cx| Root::new(editor, window, cx))
            },
        )
        .expect("open editor window");
        cx.activate(true);
    });
    Ok(())
}
