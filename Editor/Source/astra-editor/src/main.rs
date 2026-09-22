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

struct Editor {
    project: Project,
    input: Entity<InputState>,
    status: String,
    syncing: bool,
    preview_config: Option<PreviewConfig>,
    preview: Option<Preview>,
    close_confirmed: bool,
    bridge: EditorBridge,
    requests: tokio::sync::mpsc::Receiver<Request>,
    agent: AgentEdits,
    mode: EditMode,
    pending_reply: Option<tokio::sync::oneshot::Sender<anyhow::Result<()>>>,
    agent_command: Option<String>,
    agent_prompt: Entity<InputState>,
    agent_output: String,
    runtime: tokio::runtime::Handle,
    agent_task: Option<tokio::task::JoinHandle<()>>,
    diagnostic_position: Option<gpui_component::input::Position>,
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
        let input = cx.new(|cx| InputState::new(window, cx).multi_line(true));
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
                    this.diagnose();
                }
                cx.notify();
            }
        });
        let mut editor = Self {
            project,
            input,
            status: String::new(),
            syncing: false,
            preview_config,
            preview: None,
            close_confirmed: false,
            bridge,
            requests,
            agent: AgentEdits::default(),
            mode: EditMode::ReviewEachBatch,
            pending_reply: None,
            agent_command,
            agent_prompt,
            agent_output: String::new(),
            runtime,
            agent_task: None,
            diagnostic_position: None,
            _subscription: subscription,
        };
        editor.agent.begin(editor.mode).expect("new agent turn");
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
            if this
                .update_in(cx, |this, window, cx| {
                    while let Ok(request) = this.requests.try_recv() {
                        match request {
                            Request::AgentMessage { generation, text } => {
                                if generation == this.bridge.generation()
                                    && this.agent_output.len() < 16_384
                                {
                                    this.agent_output.push_str(&text);
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
                    if let Some(preview) = &mut this.preview {
                        let version = this
                            .project
                            .documents
                            .document(&this.project.active)
                            .unwrap()
                            .version;
                        if let Err(error) = preview.poll(version) {
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
        self.status = match self.project.documents.compile(Default::default()) {
            Ok(project) => format!(
                "Compiled · {} source locations",
                project.story.source_map.len()
            ),
            Err(astra_vn_editor::VnError::Diagnostic(error)) => {
                if let Some(source) = error.source {
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

    fn cancel_agent(&mut self) {
        self.bridge.cancel();
        self.agent.cancel();
        if let Some(reply) = self.pending_reply.take() {
            let _ = reply.send(Err(anyhow::anyhow!("Cancelled by user")));
        }
        let _ = self.agent.begin(self.mode);
    }

    fn prompt_agent(&mut self, cx: &mut Context<Self>) {
        if self
            .agent_task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            self.status = "Wait for the active agent or cancel it".into();
            return;
        }
        let Some(command) = self.agent_command.clone() else {
            self.status = "Configure an external agent with --agent".into();
            return;
        };
        let prompt = self.agent_prompt.read(cx).value().to_string();
        if prompt.trim().is_empty() {
            return;
        }
        self.cancel_agent();
        self.agent_output.clear();
        let source = self.project.source_path().to_path_buf();
        let bridge = self.bridge.clone();
        let generation = bridge.generation();
        self.agent_task = Some(self.runtime.spawn(async move {
            let result = astra_editor::acp::prompt(command, prompt, source, bridge.clone()).await;
            bridge
                .message(
                    generation,
                    match result {
                        Ok(()) => "\nAgent turn ended".into(),
                        Err(error) => format!("\nAgent failed: {error}"),
                    },
                )
                .await;
        }));
    }

    fn review(&mut self, approve: bool, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.agent.resolve(approve, &mut self.project.documents);
        if let Some(reply) = self.pending_reply.take() {
            let _ = reply.send(if approve {
                result
            } else {
                Err(anyhow::anyhow!("User rejected batch"))
            });
        }
        self.sync(window, cx);
    }

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let document = self
            .project
            .documents
            .document(&self.project.active)
            .unwrap();
        let dirty = self
            .project
            .documents
            .is_dirty(&self.project.active)
            .unwrap();
        div().flex().flex_col().size_full().p_4().gap_3().bg(rgb(0x181b22)).text_color(rgb(0xe4e8ef))
            .child(div().flex().gap_3().items_center()
                .child(format!("{}{} · v{}", self.project.active, if dirty { " *" } else { "" }, document.version))
                .child(Button::new("save").label("Save").on_click(cx.listener(|this, _, _, cx| {
                    this.status = this.project.save().map(|_| "Saved".to_string()).unwrap_or_else(|e| e.to_string()); cx.notify();
                })))
                .child(Button::new("undo").label("Undo batch").on_click(cx.listener(|this, _, window, cx| {
                    match this.project.documents.undo() { Ok(()) => this.sync(window, cx), Err(e) => { this.status = e.to_string(); cx.notify(); } }
                })))
                .child(Button::new("redo").label("Redo").on_click(cx.listener(|this, _, window, cx| {
                    match this.project.documents.redo() { Ok(()) => this.sync(window, cx), Err(e) => { this.status = e.to_string(); cx.notify(); } }
                })))
                .child(Button::new("preview").label("Save & Preview").on_click(cx.listener(|this, _, _, cx| {
                    let result = (|| -> anyhow::Result<()> {
                        let config = this.preview_config.as_ref().ok_or_else(|| anyhow::anyhow!("Open with a preview configuration to bind project, target, profile and built tools"))?;
                        this.project.documents.compile(Default::default())?;
                        this.project.save()?;
                        this.preview = None;
                        let version = this.project.documents.document(&this.project.active)?.version;
                        this.preview = Some(Preview::start(config, version)?);
                        Ok(())
                    })();
                    if let Err(error) = result { this.status = error.to_string(); }
                    cx.notify();
                })))
                .child(Button::new("stop").label("Stop preview").on_click(cx.listener(|this, _, _, cx| {
                    this.preview = None; cx.notify();
                }))))
            .child(div().flex_1().min_h_0().child(Input::new(&self.input).h_full()))
            .child(div().flex().gap_3().child(self.status.clone()).child(Button::new("locate-diagnostic").label("Go to diagnostic").disabled(self.diagnostic_position.is_none()).on_click(cx.listener(|this, _, window, cx| {
                if let Some(position) = this.diagnostic_position { this.input.update(cx, |input, cx| input.set_cursor_position(position, window, cx)); }
            }))))
            .child(div().flex().gap_3()
                .child(Button::new("agent-mode").label(format!("Agent: {:?}", self.mode)).on_click(cx.listener(|this, _, _, cx| {
                    this.mode = if this.mode == EditMode::Autonomous { EditMode::ReviewEachBatch } else { EditMode::Autonomous };
                    this.cancel_agent(); cx.notify();
                })))
                .child(Button::new("cancel-agent").label("Cancel agent").on_click(cx.listener(|this, _, _, cx| { this.cancel_agent(); cx.notify(); })))
                .child(Button::new("approve").label("Apply batch").disabled(self.pending_reply.is_none()).on_click(cx.listener(|this, _, window, cx| { this.review(true, window, cx); })))
                .child(Button::new("reject").label("Reject batch").disabled(self.pending_reply.is_none()).on_click(cx.listener(|this, _, window, cx| { this.review(false, window, cx); }))))
            .child(self.agent.pending().map(|batch| serde_json::to_string_pretty(batch).unwrap_or_default()).unwrap_or_default())
            .child(div().flex().gap_3().child(Input::new(&self.agent_prompt)).child(Button::new("send-agent").label("Send").on_click(cx.listener(|this, _, _, cx| { this.prompt_agent(cx); cx.notify(); }))))
            .child(self.agent_output.clone())
            .child(self.preview.as_ref().map(|p| p.status.clone()).unwrap_or_default())
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
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let project = Project::open(&args.source)?;
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
                        if editor.close_confirmed
                            || !editor
                                .project
                                .documents
                                .is_dirty(&editor.project.active)
                                .unwrap_or(true)
                        {
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
                                    if let Err(error) = this.project.save() {
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
