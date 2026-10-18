use astra_emu_abi as abi;
use astra_emu_manager_core::{
    extensions::LocalExtension,
    model::{FamilyError, FamilyResult},
};
use astra_emu_translation_openai_compatible::{
    OpenAiCompatibleTranslationProvider, SecretResolver, TranslationProfile, TranslationRequest,
    TranslationSegment, TranslationSession,
};
use crossbeam_channel::{Sender, bounded};
use mlua::LuaSerdeExt;
use parking_lot::{Condvar, Mutex};
use std::{
    ffi::c_void,
    sync::{
        Arc, LazyLock, Weak,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

const MAX_ITEMS: usize = 128;
const MAX_TEXT: usize = 1024 * 1024;
const TRANSLATION_LUA: &str = "return function(item, config) return astra.translate(item.text) end";
#[derive(Clone)]
struct Item {
    id: u64,
    kind: u32,
    text: String,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    session: u64,
    context: u64,
    request: u64,
}
struct Completed {
    key: Key,
    bytes: Vec<u8>,
    ranges: Vec<abi::TextRange>,
}
struct Pending {
    key: Key,
    finished: Option<FamilyResult<Completed>>,
}
struct State {
    epoch: u64,
    context: AtomicU64,
    closed: AtomicBool,
    active: AtomicUsize,
    pending: Mutex<Option<Pending>>,
    changed: Condvar,
    provider: Option<Arc<TranslationSession<dyn SecretResolver>>>,
    extensions: Vec<LocalExtension>,
    recent: Mutex<std::collections::VecDeque<TranslationSegment>>,
    queue: Sender<Job>,
}
enum Job {
    Text(Arc<State>, Key, Vec<Item>),
    Stop,
}
struct Worker {
    sender: Sender<Job>,
    thread: Mutex<Option<JoinHandle<()>>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.sender.send(Job::Stop);
        if let Some(thread) = self.thread.get_mut().take() {
            let _ = thread.join();
        }
    }
}
static WORKER: LazyLock<Mutex<Weak<Worker>>> = LazyLock::new(|| Mutex::new(Weak::new()));
fn worker() -> FamilyResult<Arc<Worker>> {
    let mut shared = WORKER.lock();
    if let Some(worker) = shared.upgrade() {
        return Ok(worker);
    }
    let (sender, receiver) = bounded(64);
    let (ready, initialized) = bounded(1);
    let thread = std::thread::Builder::new()
        .name("astra-extensions-luau".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(v) => v,
                Err(_) => {
                    let _ = ready.send(Err(error("ASTRA_EMU_EXTENSION_RUNTIME")));
                    return;
                }
            };
            let lua = mlua::Lua::new();
            if lua.set_memory_limit(64 * 1024 * 1024).is_err() {
                let _ = ready.send(Err(error("ASTRA_EMU_EXTENSION_MEMORY")));
                return;
            }
            let _ = ready.send(Ok(()));
            let mut compiled = None;
            while let Ok(job) = receiver.recv() {
                let Job::Text(state, key, items) = job else {
                    break;
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    execute(&lua, &runtime, &state, key, items, &mut compiled)
                }))
                .unwrap_or_else(|_| Err(error("ASTRA_EMU_EXTENSION_PANIC")));
                let mut pending = state.pending.lock();
                if let Some(pending) = pending.as_mut().filter(|pending| pending.key == key) {
                    pending.finished = Some(result);
                }
                state.active.fetch_sub(1, Ordering::AcqRel);
                state.changed.notify_all();
            }
        })
        .map_err(|_| error("ASTRA_EMU_EXTENSION_THREAD"))?;
    let worker = Arc::new(Worker {
        sender,
        thread: Mutex::new(Some(thread)),
    });
    initialized
        .recv()
        .map_err(|_| error("ASTRA_EMU_EXTENSION_THREAD"))??;
    *shared = Arc::downgrade(&worker);
    Ok(worker)
}
#[derive(Clone)]
pub(crate) struct TextReplacementBridge {
    state: Arc<State>,
    _worker: Arc<Worker>,
}
impl TextReplacementBridge {
    pub(crate) fn new(
        epoch: u64,
        profile: Option<TranslationProfile>,
        secrets: Arc<dyn SecretResolver>,
        mut extensions: Vec<LocalExtension>,
    ) -> FamilyResult<Self> {
        let provider = if let Some(profile) = profile {
            profile
                .validate()
                .map_err(|_| error("ASTRA_EMU_TEXT_PROFILE"))?;
            extensions.insert(
                0,
                LocalExtension {
                    id: "builtin-translation".into(),
                    enabled: true,
                    source: TRANSLATION_LUA.into(),
                    configuration: serde_json::json!({}),
                },
            );
            Some(Arc::new(TranslationSession::new(Arc::new(
                OpenAiCompatibleTranslationProvider::new(profile, secrets)
                    .map_err(|_| error("ASTRA_EMU_TEXT_PROVIDER"))?,
            ))))
        } else {
            None
        };
        extensions.retain(|entry| entry.enabled);
        let worker = worker()?;
        Ok(Self {
            state: Arc::new(State {
                epoch,
                context: AtomicU64::new(0),
                closed: AtomicBool::new(false),
                active: AtomicUsize::new(0),
                pending: Mutex::new(None),
                changed: Condvar::new(),
                provider,
                extensions,
                recent: Mutex::new(std::collections::VecDeque::new()),
                queue: worker.sender.clone(),
            }),
            _worker: worker,
        })
    }
    pub(crate) fn sink(&self) -> abi::TextService {
        abi::TextService {
            context: Arc::as_ptr(&self.state).cast_mut().cast(),
            request: Some(request),
            acknowledge: Some(acknowledge),
            invalidate: Some(invalidate),
        }
    }
    pub(crate) fn cancel(&self) {
        self.state.closed.store(true, Ordering::Release);
        self.state.pending.lock().take();
        self.state.changed.notify_all();
    }
    pub(crate) fn close(&self) -> FamilyResult<()> {
        self.state.closed.store(true, Ordering::Release);
        let mut pending = self.state.pending.lock();
        pending.take();
        self.state.changed.notify_all();
        while self.state.active.load(Ordering::Acquire) != 0 {
            self.state.changed.wait(&mut pending);
        }
        let provider_result = self.state.provider.as_ref().map_or(Ok(()), |provider| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| provider.clear()))
                .map_err(|_| error("ASTRA_EMU_TEXT_PROVIDER_PANIC"))
        });
        self.state.recent.lock().clear();
        provider_result
    }
}
fn cancelled(state: &State, key: Key) -> bool {
    state.closed.load(Ordering::Acquire)
        || state.epoch != key.session
        || state.context.load(Ordering::Acquire) != key.context
}
struct CompiledScripts {
    epoch: u64,
    chunks: Vec<(mlua::Function, mlua::Value)>,
}
fn execute(
    lua: &mlua::Lua,
    runtime: &tokio::runtime::Runtime,
    state: &Arc<State>,
    key: Key,
    mut items: Vec<Item>,
    compiled: &mut Option<CompiledScripts>,
) -> FamilyResult<Completed> {
    if cancelled(state, key) {
        return Err(error("ASTRA_EMU_TEXT_CANCELLED"));
    }
    let cancellation = state.clone();
    lua.set_interrupt(move |_| {
        if cancelled(&cancellation, key) {
            Err(mlua::Error::RuntimeError("ASTRA_EMU_TEXT_CANCELLED".into()))
        } else {
            Ok(mlua::VmState::Continue)
        }
    });
    let native_error = Mutex::new(None);
    let speaker = items
        .iter()
        .find(|item| item.kind == abi::TEXT_SPEAKER)
        .map(|item| item.text.clone());
    let result = lua.scope(|scope| {
        if compiled.as_ref().is_none_or(|compiled| compiled.epoch != state.epoch) {
            let chunks = state.extensions.iter().map(|extension| Ok((lua.load(&extension.source).set_name(&extension.id).into_function()?, lua.to_value(&extension.configuration)?))).collect::<mlua::Result<Vec<_>>>()?;
            *compiled = Some(CompiledScripts { epoch: state.epoch, chunks });
        }
        let api = lua.create_table()?;
        let translate = scope.create_function(|_, text: String| {
            let provider = state.provider.as_ref().ok_or_else(|| mlua::Error::RuntimeError("ASTRA_EMU_TEXT_PROFILE_REQUIRED".into()))?;
            let request = TranslationRequest { current: TranslationSegment { text, speaker: speaker.clone(), ruby: None }, recent: state.recent.lock().iter().cloned().collect() };
            let translated = runtime.block_on(async {
                let future = provider.translate(&request);
                tokio::pin!(future);
                loop {
                    if cancelled(state, key) { return Err(error("ASTRA_EMU_TEXT_CANCELLED")); }
                    tokio::select! { result = &mut future => return result.map_err(translation_error), _ = tokio::time::sleep(Duration::from_millis(5)) => {} }
                }
            }).map_err(|error| { let code = error.code().to_owned(); *native_error.lock() = Some(error); mlua::Error::RuntimeError(code) })?;
            let mut recent = state.recent.lock();
            if cancelled(state, key) { return Err(mlua::Error::RuntimeError("ASTRA_EMU_TEXT_CANCELLED".into())); }
            recent.push_back(request.current);
            while recent.len() > astra_emu_translation_openai_compatible::MAX_CONTEXT_SEGMENTS
                || recent.iter().map(|segment| segment.text.chars().count() + segment.speaker.as_deref().map_or(0, |speaker| speaker.chars().count() + 8)).sum::<usize>() > astra_emu_translation_openai_compatible::MAX_CONTEXT_CHARS {
                recent.pop_front();
            }
            Ok(translated.translated)
        })?;
        api.set("translate", translate)?;
        lua.globals().set("astra", api)?;
        for (chunk, configuration) in &compiled.as_ref().expect("compiled for current epoch").chunks {
            let transform: mlua::Function = chunk.call(())?;
            for item in &mut items {
                let input = lua.create_table()?;
                input.set("id", item.id)?; input.set("kind", item.kind)?; input.set("text", item.text.as_str())?;
                input.set("session_epoch", key.session)?; input.set("context_epoch", key.context)?; input.set("request_id", key.request)?;
                let output: String = transform.call((input, configuration.clone()))?;
                if output.len() > MAX_TEXT || output.contains('\0') { return Err(mlua::Error::RuntimeError("ASTRA_EMU_TEXT_RESULT_BOUNDS".into())); }
                item.text = output;
            }
        }
        Ok(())
    });
    lua.remove_interrupt();
    lua.globals()
        .set("astra", mlua::Value::Nil)
        .map_err(|_| error("ASTRA_EMU_EXTENSION_LUA"))?;
    result.map_err(|_| {
        native_error.lock().take().unwrap_or_else(|| {
            error(if cancelled(state, key) {
                "ASTRA_EMU_TEXT_CANCELLED"
            } else {
                "ASTRA_EMU_EXTENSION_LUA"
            })
        })
    })?;
    if cancelled(state, key) {
        return Err(error("ASTRA_EMU_TEXT_CANCELLED"));
    }
    let total = items
        .iter()
        .try_fold(0usize, |n, item| n.checked_add(item.text.len()))
        .filter(|&n| n <= MAX_TEXT)
        .ok_or_else(|| error("ASTRA_EMU_TEXT_RESULT_BOUNDS"))?;
    let mut result = Completed {
        key,
        bytes: Vec::with_capacity(total),
        ranges: Vec::with_capacity(items.len()),
    };
    for item in items {
        result.ranges.push(abi::TextRange {
            id: item.id,
            offset: result.bytes.len(),
            len: item.text.len(),
        });
        result.bytes.extend_from_slice(item.text.as_bytes());
    }
    Ok(result)
}
unsafe extern "C" fn request(
    context: *mut c_void,
    raw: *const abi::TextRequest,
    output: *mut abi::TextResult,
    error_out: *mut abi::Error,
) -> abi::Status {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        request_inner(context, raw, output)
    })) {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            if let Some(out) = unsafe { error_out.as_mut() } {
                out.set(error.code(), &error.message);
            }
            if error.code().contains("CANCELLED") {
                abi::CANCELLED
            } else if error.code().ends_with("PANIC") {
                abi::PANICKED
            } else {
                abi::CORE_ERROR
            }
        }
        Err(_) => {
            if let Some(out) = unsafe { error_out.as_mut() } {
                out.set("ASTRA_EMU_EXTENSION_PANIC", "text service panicked");
            }
            abi::PANICKED
        }
    }
}
unsafe fn request_inner(
    context: *mut c_void,
    raw: *const abi::TextRequest,
    output: *mut abi::TextResult,
) -> FamilyResult<abi::Status> {
    let state = unsafe { context.cast::<State>().as_ref() }
        .ok_or_else(|| error("ASTRA_EMU_TEXT_REQUEST"))?;
    let raw = unsafe { raw.as_ref() }.ok_or_else(|| error("ASTRA_EMU_TEXT_REQUEST"))?;
    let output = unsafe { output.as_mut() }.ok_or_else(|| error("ASTRA_EMU_TEXT_REQUEST"))?;
    let key = Key {
        session: raw.session_epoch,
        context: raw.context_epoch,
        request: raw.request_id,
    };
    if key.context == 0 {
        return Err(error("ASTRA_EMU_TEXT_CONTEXT"));
    }
    let _ = state
        .context
        .compare_exchange(0, key.context, Ordering::AcqRel, Ordering::Acquire);
    if key.request == 0 || cancelled(state, key) {
        return Err(error("ASTRA_EMU_TEXT_CANCELLED"));
    }
    let mut pending = state.pending.lock();
    if cancelled(state, key) {
        return Err(error("ASTRA_EMU_TEXT_CANCELLED"));
    }
    if let Some(existing) = pending.as_ref() {
        if existing.key != key {
            return Err(error("ASTRA_EMU_TEXT_BUSY"));
        }
    } else {
        let raw_items = unsafe { abi::borrowed_array(raw.items, raw.item_count, MAX_ITEMS) }
            .map_err(|_| error("ASTRA_EMU_TEXT_REQUEST"))?;
        if raw_items.is_empty() {
            return Err(error("ASTRA_EMU_TEXT_REQUEST"));
        }
        let mut items = Vec::with_capacity(raw_items.len());
        let mut total = 0usize;
        for (i, item) in raw_items.iter().enumerate() {
            if !matches!(
                item.kind,
                abi::TEXT_DIALOGUE | abi::TEXT_SPEAKER | abi::TEXT_CHOICE
            ) || item.reserved != 0
                || raw_items[..i].iter().any(|prior| prior.id == item.id)
            {
                return Err(error("ASTRA_EMU_TEXT_REQUEST"));
            }
            let text =
                unsafe { item.text.text(MAX_TEXT) }.map_err(|_| error("ASTRA_EMU_TEXT_REQUEST"))?;
            total += text.len();
            if total > MAX_TEXT {
                return Err(error("ASTRA_EMU_TEXT_REQUEST"));
            }
            items.push(Item {
                id: item.id,
                kind: item.kind,
                text: text.to_owned(),
            });
        }
        *pending = Some(Pending {
            key,
            finished: None,
        });
        unsafe {
            Arc::increment_strong_count(context.cast::<State>());
        }
        let owned = unsafe { Arc::from_raw(context.cast::<State>()) };
        state.active.fetch_add(1, Ordering::AcqRel);
        if state.queue.try_send(Job::Text(owned, key, items)).is_err() {
            state.active.fetch_sub(1, Ordering::AcqRel);
            pending.take();
            return Err(error("ASTRA_EMU_EXTENSION_QUEUE"));
        }
    }
    loop {
        if cancelled(state, key) {
            pending.take();
            return Err(error("ASTRA_EMU_TEXT_CANCELLED"));
        }
        let current = pending
            .as_mut()
            .ok_or_else(|| error("ASTRA_EMU_TEXT_CANCELLED"))?;
        if current.finished.is_some() {
            break;
        }
        state.changed.wait(&mut pending);
    }
    if pending
        .as_ref()
        .and_then(|p| p.finished.as_ref())
        .is_some_and(Result::is_err)
    {
        return Err(pending.take().unwrap().finished.unwrap().err().unwrap());
    }
    let completed = pending
        .as_ref()
        .unwrap()
        .finished
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    output.byte_count = completed.bytes.len();
    output.item_count = completed.ranges.len();
    if output.byte_capacity < output.byte_count || output.item_capacity < output.item_count {
        return Ok(abi::CAPACITY);
    }
    if (output.byte_count != 0 && output.bytes.is_null())
        || output.items.is_null()
        || !output.items.is_aligned()
    {
        return Err(error("ASTRA_EMU_TEXT_RESULT_BUFFER"));
    }
    unsafe {
        if !completed.bytes.is_empty() {
            std::ptr::copy_nonoverlapping(
                completed.bytes.as_ptr(),
                output.bytes,
                completed.bytes.len(),
            );
        }
        std::ptr::copy_nonoverlapping(
            completed.ranges.as_ptr(),
            output.items,
            completed.ranges.len(),
        );
    }
    Ok(abi::OK)
}
unsafe extern "C" fn acknowledge(
    context: *mut c_void,
    session: u64,
    epoch: u64,
    request: u64,
) -> abi::Status {
    let Some(state) = (unsafe { context.cast::<State>().as_ref() }) else {
        return abi::INVALID_ARGUMENT;
    };
    let key = Key {
        session,
        context: epoch,
        request,
    };
    if cancelled(state, key) {
        return abi::STALE_CONTEXT;
    }
    let mut pending = state.pending.lock();
    if pending
        .as_ref()
        .and_then(|p| p.finished.as_ref())
        .and_then(|r| r.as_ref().ok())
        .is_some_and(|r| r.key == key)
    {
        pending.take();
        abi::OK
    } else {
        abi::INVALID_STATE
    }
}
unsafe extern "C" fn invalidate(context: *mut c_void, session: u64, epoch: u64) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(state) = (unsafe { context.cast::<State>().as_ref() }) else {
            return;
        };
        if session != state.epoch {
            return;
        }
        state.context.store(epoch, Ordering::Release);
        state.pending.lock().take();
        state.changed.notify_all();
        if let Some(provider) = &state.provider {
            provider.clear();
        }
        state.recent.lock().clear();
    }));
}
fn error(code: &str) -> FamilyError {
    FamilyError::new(code, "text extension operation failed")
}
fn translation_error(
    value: astra_emu_translation_openai_compatible::TranslationError,
) -> FamilyError {
    use astra_emu_translation_openai_compatible::TranslationError as E;
    error(match value {
        E::InvalidProfile(_) => "ASTRA_EMU_TEXT_PROFILE",
        E::InvalidRequest(_) => "ASTRA_EMU_TEXT_REQUEST",
        E::ContextLimit => "ASTRA_EMU_TEXT_CONTEXT_LIMIT",
        E::SecretUnavailable | E::HttpStatus(401 | 403) => "ASTRA_EMU_TEXT_AUTH",
        E::Timeout => "ASTRA_EMU_TEXT_TIMEOUT",
        E::HttpStatus(429) => "ASTRA_EMU_TEXT_RATE_LIMITED",
        E::HttpStatus(_) | E::Transport => "ASTRA_EMU_TEXT_NETWORK",
        E::Protocol(_) => "ASTRA_EMU_TEXT_PROTOCOL",
        E::SessionReset => "ASTRA_EMU_TEXT_CANCELLED",
    })
}

#[cfg(test)]
#[path = "text_service_tests.rs"]
mod tests;

/// Application-scoped owner keeps the shared serial VM on one thread across
/// native session close/reopen, then joins it during Manager teardown.
pub(crate) struct ExtensionRuntime {
    _worker: Arc<Worker>,
}
impl ExtensionRuntime {
    pub(crate) fn new() -> FamilyResult<Self> {
        Ok(Self { _worker: worker()? })
    }
}
