use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticPass {
    Symbols,
    Routes,
    Variables,
    Commands,
    SystemStories,
    CompiledStory,
}

pub const SEMANTIC_PASS_ORDER: [SemanticPass; 6] = [
    SemanticPass::Symbols,
    SemanticPass::Routes,
    SemanticPass::Variables,
    SemanticPass::Commands,
    SemanticPass::SystemStories,
    SemanticPass::CompiledStory,
];

pub(super) fn trace_pass(pass: SemanticPass) {
    tracing::debug!(
        event = "vn.compile.semantic_pass",
        pass = ?pass,
        "AstraVN semantic pass started"
    );
}

pub(super) fn compile_bound_sources(
    sources: Vec<AstraSource>,
    registry: &CommandRegistry,
) -> Result<CompiledStory, VnError> {
    tracing::info!(event = "vn.compile.start", "AstraVN compilation started");
    let lines = lower_sources_from_cst(&sources);
    let mut builder = CompileBuilder::default();
    for line in &lines {
        builder.validate_structure(line)?;
        builder.record_source(line)?;
        match line.keyword.as_str() {
            "story" => builder.start_story(line)?,
            "state" => builder.start_state(line)?,
            "scene" => builder.start_scene(line)?,
            "text" => builder.push_dialogue(line)?,
            "choice" => builder.push_choice(line)?,
            "option" => builder.push_option(line)?,
            "jump" => builder.push_jump(line)?,
            "branch" => builder.push_branch(line)?,
            "call" => builder.push_call(line)?,
            "return" => builder.push_return(line)?,
            "mutate" => builder.push_mutate(line)?,
            "system_page" => builder.push_system_page(line)?,
            "system_action" => builder.push_system_action(line)?,
            "wait" => builder.push_wait(line)?,
            "input_wait" => builder.push_input_wait(line)?,
            _ => match registry.provider(&line.keyword) {
                Some(CommandProvider::Standard) => builder.push_stage_presentation(line)?,
                Some(CommandProvider::Extension(descriptor)) => {
                    builder.push_extension_presentation(line, descriptor)?;
                }
                Some(CommandProvider::Core) => {
                    return Err(VnError::Diagnostic(
                        Diagnostic::blocking(
                            "ASTRA_VN_CORE_COMMAND_UNHANDLED",
                            "core command has no compiler lowering",
                        )
                        .with_source(line.source_ref())
                        .with_field("command", &line.keyword),
                    ));
                }
                None => {
                    return Err(VnError::Diagnostic(
                        Diagnostic::blocking(
                            "ASTRA_VN_COMMAND_UNBOUND",
                            "command requires an explicit provider binding before compile",
                        )
                        .with_source(line.source_ref())
                        .with_field("command", &line.keyword),
                    ));
                }
            },
        }
    }
    trace_pass(SemanticPass::Symbols);
    let mut compiled = builder.finish()?;
    trace_pass(SemanticPass::Commands);
    trace_pass(SemanticPass::SystemStories);
    compiled.system_story_manifest = SystemStoryManifest::from_compiled(&compiled)?;
    trace_pass(SemanticPass::CompiledStory);
    let bytes = postcard::to_allocvec(&semantic_story(&compiled))?;
    compiled.story_hash = Hash128::from_blake3(&bytes);
    enrich_source_map(&mut compiled, &sources);
    compiled.source_map.refresh_hash();
    tracing::info!(
        event = "vn.compile.complete",
        story_count = compiled.stories.len(),
        state_count = compiled.states.len(),
        source_entry_count = compiled.source_map.len(),
        "AstraVN compilation completed"
    );
    Ok(compiled)
}

pub(super) fn enrich_source_map(compiled: &mut CompiledStory, sources: &[AstraSource]) {
    for source in sources {
        let line_index = SourceLineIndex::new(&source.text);
        let parsed = crate::parse_astra_source(source.path.clone(), &source.text);
        for command in parsed.ast.commands() {
            let id = command.source_id().map(str::to_string).unwrap_or_else(|| {
                format!("{}:{}:{}", source.path, command.line(), command.keyword())
            });
            let Some(entry) = compiled.source_map.entries.get_mut(&id) else {
                continue;
            };
            entry.keyword = line_index.source_ref(&source.path, command.keyword_span());
            entry.source_id = command
                .source_id_span()
                .map(|span| line_index.source_ref(&source.path, span));
            entry.attributes = command
                .attributes()
                .map(|attribute| {
                    (
                        attribute.key().to_string(),
                        line_index.source_ref(&source.path, attribute.value_span),
                    )
                })
                .collect();
            entry.arguments = command
                .arguments()
                .map(|(_, span)| line_index.source_ref(&source.path, span))
                .collect();
        }
    }
}

pub(super) struct SourceLineIndex {
    pub(super) line_starts: Vec<usize>,
    pub(super) text_len: usize,
}

impl SourceLineIndex {
    pub(super) fn new(text: &str) -> Self {
        let mut line_starts =
            Vec::with_capacity(text.bytes().filter(|byte| *byte == b'\n').count() + 1);
        line_starts.push(0);
        line_starts.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        Self {
            line_starts,
            text_len: text.len(),
        }
    }

    pub(super) fn source_ref(&self, path: &str, span: crate::TextSpan) -> SourceRef {
        let start = (u32::from(span.start) as usize).min(self.text_len);
        let line_index = self
            .line_starts
            .partition_point(|line_start| *line_start <= start)
            - 1;
        SourceRef {
            source: path.to_string(),
            line: line_index as u32 + 1,
            column: (start - self.line_starts[line_index]) as u32 + 1,
            length: u32::from(span.end - span.start),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CompileAstraProjectOptions {
    pub(super) extension_bindings: Vec<ExtensionCommandDescriptor>,
    pub(super) ui_themes: Vec<astra_ui_core::UiThemeManifest>,
    pub(super) ui_controller_sources: BTreeMap<String, String>,
}

impl CompileAstraProjectOptions {
    pub fn bind_extension(mut self, descriptor: ExtensionCommandDescriptor) -> Self {
        self.extension_bindings.push(descriptor);
        self
    }

    pub fn with_ui_theme(mut self, theme: astra_ui_core::UiThemeManifest) -> Self {
        self.ui_themes.push(theme);
        self
    }

    pub fn with_ui_controller_source(
        mut self,
        controller_id: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        self.ui_controller_sources
            .insert(controller_id.into(), source.into());
        self
    }
}

pub(super) fn compile_story_sources_with_options<I, S>(
    sources: I,
    options: CompileAstraProjectOptions,
) -> Result<CompiledStory, VnError>
where
    I: IntoIterator<Item = S>,
    S: Into<AstraSource>,
{
    let sources: Vec<AstraSource> = sources.into_iter().map(Into::into).collect();
    for source in &sources {
        let parsed = crate::parse_astra_source(source.path.clone(), &source.text);
        if let Some(diagnostic) = parsed
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code != "ASTRA_VN_UNKNOWN_COMMAND")
        {
            let mut diagnostic = diagnostic.clone();
            diagnostic.severity = DiagnosticSeverity::Blocking;
            return Err(VnError::Diagnostic(diagnostic));
        }
    }
    let mut registry = CommandRegistry::default();
    for descriptor in options.extension_bindings {
        registry.bind_extension(descriptor)?;
    }
    compile_bound_sources(sources, &registry)
}

pub fn compile_astra_project<I, S>(
    sources: I,
    options: CompileAstraProjectOptions,
) -> Result<CompiledVnProject, VnError>
where
    I: IntoIterator<Item = S>,
    S: Into<AstraSource>,
{
    let sources: Vec<AstraSource> = sources.into_iter().map(Into::into).collect();
    let story_sources: Vec<AstraSource> = sources
        .iter()
        .filter(|source| source.role == AstraSourceRole::Story)
        .cloned()
        .collect();
    let ui_sources: Vec<AstraSource> = sources
        .iter()
        .filter(|source| source.role == AstraSourceRole::Ui)
        .cloned()
        .collect();
    if story_sources.is_empty() {
        return Err(VnError::message(
            "ASTRA_VN_STORY_SOURCE_MISSING: project requires at least one Story source",
        ));
    }
    let ui_themes = options.ui_themes.clone();
    let ui_controller_sources = options.ui_controller_sources.clone();
    let story = compile_story_sources_with_options(story_sources, options)?;
    crate::ui_compile::compile_project_ui(story, &ui_sources, ui_themes, ui_controller_sources)
}

#[derive(Default)]
pub(super) struct CompileBuilder {
    pub(super) stories: Vec<Story>,
    pub(super) states: BTreeMap<String, State>,
    pub(super) current_story: Option<String>,
    pub(super) current_state: Option<String>,
    pub(super) current_scene: Option<String>,
    pub(super) source_map: CommandSourceMap,
    pub(super) debug_symbols: BTreeMap<String, String>,
    pub(super) system_actions: BTreeMap<String, SystemActionProgram>,
}
