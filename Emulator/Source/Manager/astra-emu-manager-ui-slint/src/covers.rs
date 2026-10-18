use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, atomic::{AtomicU64, Ordering}},
    thread::JoinHandle,
    time::SystemTime,
};

use parking_lot::{Condvar, Mutex};
use slint::{Model, ModelTracker, VecModel};
use crate::{GameCard, ManagerWindow};

const IN_FLIGHT: usize = 4;
const CACHE_SIZE: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    path: PathBuf,
    length: u64,
    modified: SystemTime,
}

#[derive(Clone, Debug)]
struct Request {
    revision: u64,
    row: usize,
    case_id: String,
    locator: String,
    previous: Option<Stamp>,
}

struct Pixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

enum Decoded {
    Missing,
    Unchanged(Stamp),
    Image(Stamp, Pixels),
    Failed(&'static str),
}

#[derive(Default)]
struct Queue {
    stopped: bool,
    pending: VecDeque<Request>,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    revision: AtomicU64,
}

impl Shared {
    fn cancel(&self, revision: u64) {
        self.revision.store(revision, Ordering::Release);
        self.queue.lock().pending.clear();
    }

    fn current(&self, request: &Request) -> bool {
        self.revision.load(Ordering::Acquire) == request.revision
    }

    fn push(&self, request: Request) {
        let mut queue = self.queue.lock();
        // The UI scheduler counts running and queued work together.
        assert!(queue.pending.len() < IN_FLIGHT);
        if !queue.stopped {
            queue.pending.push_back(request);
            self.wake.notify_one();
        }
    }

    fn next(&self) -> Option<Request> {
        let mut queue = self.queue.lock();
        loop {
            if queue.stopped {
                return None;
            }
            if let Some(request) = queue.pending.pop_front() {
                return Some(request);
            }
            self.wake.wait(&mut queue);
        }
    }
}

struct Cached {
    locator: String,
    stamp: Stamp,
    image: slint::Image,
}

#[derive(Default)]
struct Demand {
    revision: u64,
    visible: BTreeMap<String, HashMap<usize, String>>,
    desired: BTreeSet<usize>,
    completed: HashSet<usize>,
    in_flight: HashSet<usize>,
    cache: VecDeque<Cached>,
}

impl Demand {
    fn record_visible(&mut self, row: usize, case_id: &str, needed: bool, view: &str) {
        let visible = self.visible.entry(view.to_owned()).or_default();
        if needed {
            visible.insert(row, case_id.to_owned());
        } else if visible.get(&row).is_some_and(|id| id == case_id) {
            visible.remove(&row);
        }
    }

    fn accepts(&self, request: &Request, card: Option<&GameCard>) -> bool {
        request.revision == self.revision
            && self.desired.contains(&request.row)
            && card.is_some_and(|card| {
                card.case_id.as_str() == request.case_id
                    && card.cover_uri.as_str() == request.locator
            })
    }

    fn next_rows(&self) -> impl Iterator<Item = usize> + '_ {
        self.desired.iter().copied()
            .filter(|row| !self.completed.contains(row) && !self.in_flight.contains(row))
            .take(IN_FLIGHT.saturating_sub(self.in_flight.len()))
    }
}

/// Owns the UI-only images and one joined decoder. Model downcasting lets the
/// event-loop completion reach this owner without sending Rc or Image across threads.
pub(crate) struct CoverModel {
    rows: VecModel<GameCard>,
    indices: RefCell<HashMap<String, usize>>,
    demand: RefCell<Demand>,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl CoverModel {
    pub(crate) fn new(window: slint::Weak<ManagerWindow>) -> Result<Self, slint::PlatformError> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            revision: AtomicU64::new(0),
        });
        let worker_shared = shared.clone();
        let worker = std::thread::Builder::new().name("astra-cover-decode".into()).spawn(move || {
            let mut diagnostics = 0_u32;
            while let Some(request) = worker_shared.next() {
                if !worker_shared.current(&request) {
                    continue;
                }
                let decoded = decode(&request);
                if !worker_shared.current(&request) {
                    continue;
                }
                if let Decoded::Failed(code) = &decoded {
                    if diagnostics < 16 {
                        tracing::warn!(target: "astra.emu.cover", code = *code, "Cover thumbnail unavailable");
                        diagnostics += 1;
                    }
                }
                let weak = window.clone();
                if slint::invoke_from_event_loop(move || {
                    if let Some(window) = weak.upgrade() {
                        let model = window.get_games();
                        if let Some(covers) = model.as_any().downcast_ref::<CoverModel>() {
                            covers.complete(request, decoded, &window);
                        }
                    }
                }).is_err() {
                    // Event-loop teardown is terminal; the model still joins this worker.
                    break;
                }
            }
        }).map_err(|_| slint::PlatformError::Other("Cannot start cover decoder".into()))?;
        Ok(Self { rows: VecModel::default(), indices: RefCell::default(), demand: RefCell::default(), shared, worker: Some(worker) })
    }

    pub(crate) fn replace(&self, cards: Vec<GameCard>, window: &ManagerWindow) {
        self.reset();
        *self.indices.borrow_mut() = cards.iter().enumerate()
            .map(|(row, card)| (card.case_id.to_string(), row)).collect();
        self.rows.set_vec(cards);
        self.demand.borrow_mut().visible.values_mut().for_each(|visible| {
            visible.retain(|_, id| self.indices.borrow().contains_key(id));
        });
        self.update_demand(window);
    }

    pub(crate) fn refresh(&self, window: &ManagerWindow) {
        // Revalidate metadata off-thread even when a locator did not change.
        if self.demand.borrow().in_flight.is_empty() {
            self.reset();
        }
        self.update_demand(window);
    }

    fn reset(&self) {
        let mut demand = self.demand.borrow_mut();
        demand.revision += 1;
        demand.completed.clear();
        demand.in_flight.clear();
        self.shared.cancel(demand.revision);
    }

    pub(crate) fn demand(&self, row: i32, case_id: &str, needed: bool, view: &str, window: &ManagerWindow) {
        if row >= 0 && !case_id.is_empty() {
            self.demand.borrow_mut().record_visible(row as usize, case_id, needed, view);
        }
        self.update_demand(window);
    }

    fn update_demand(&self, window: &ManagerWindow) {
        let enabled = window.get_cover_loading_enabled()
            && window.get_current_page() == "library" && !window.get_game_active();
        let mut desired = BTreeSet::new();
        if enabled {
            let demand = self.demand.borrow();
            if let Some(visible) = demand.visible.get(window.get_view_mode().as_str()) {
                let indices = self.indices.borrow();
                desired.extend(visible.values().filter_map(|id| indices.get(id).copied()));
            }
            let selected = window.get_selected_case_id();
            if !selected.is_empty() {
                if let Some(row) = self.indices.borrow().get(selected.as_str()) {
                    desired.insert(*row);
                }
            }
        }
        let removed = {
            let mut demand = self.demand.borrow_mut();
            let removed = demand.desired.difference(&desired).copied().collect::<Vec<_>>();
            for row in &removed {
                demand.completed.remove(row);
            }
            if demand.desired != desired {
                demand.revision += 1;
                demand.in_flight.clear();
                self.shared.cancel(demand.revision);
            }
            demand.desired = desired;
            removed
        };
        for row in removed {
            if let Some(mut card) = self.rows.row_data(row) {
                card.cover = slint::Image::default();
                self.rows.set_row_data(row, card);
            }
        }
        self.update_inspector(window);
        self.pump();
    }

    fn pump(&self) {
        let mut demand = self.demand.borrow_mut();
        loop {
            let Some(row) = demand.next_rows().next() else { break };
            let Some(card) = self.rows.row_data(row) else { break };
            if card.cover_uri.is_empty() {
                demand.completed.insert(row);
                continue;
            }
            let previous = demand.cache.iter().find(|entry| entry.locator == card.cover_uri.as_str())
                .map(|entry| entry.stamp.clone());
            self.shared.push(Request {
                revision: demand.revision, row, case_id: card.case_id.to_string(),
                locator: card.cover_uri.to_string(), previous,
            });
            demand.in_flight.insert(row);
        }
    }

    fn complete(&self, request: Request, decoded: Decoded, window: &ManagerWindow) {
        let Some(mut card) = self.rows.row_data(request.row) else { return };
        let image = {
            let mut demand = self.demand.borrow_mut();
            if request.revision != demand.revision {
                return;
            }
            demand.in_flight.remove(&request.row);
            if !demand.accepts(&request, Some(&card)) {
                drop(demand);
                self.pump();
                return;
            }
            demand.completed.insert(request.row);
            match decoded {
                Decoded::Image(stamp, pixels) => {
                    // Only the bounded thumbnail crosses into Slint's shared allocation.
                    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                        &pixels.rgba, pixels.width, pixels.height,
                    );
                    let image = slint::Image::from_rgba8(buffer);
                    demand.cache.retain(|entry| entry.locator != request.locator);
                    demand.cache.push_back(Cached { locator: request.locator, stamp, image: image.clone() });
                    while demand.cache.len() > CACHE_SIZE { demand.cache.pop_front(); }
                    image
                }
                Decoded::Unchanged(stamp) => {
                    if let Some(index) = demand.cache.iter().position(|entry| entry.locator == request.locator && entry.stamp == stamp) {
                        let entry = demand.cache.remove(index).unwrap();
                        let image = entry.image.clone();
                        demand.cache.push_back(entry);
                        image
                    } else {
                        // An LRU eviction during validation needs a decode, not a lost cover.
                        demand.completed.remove(&request.row);
                        slint::Image::default()
                    }
                }
                Decoded::Missing | Decoded::Failed(_) => {
                    demand.cache.retain(|entry| entry.locator != request.locator);
                    slint::Image::default()
                }
            }
        };
        card.cover = image;
        self.rows.set_row_data(request.row, card);
        self.update_inspector(window);
        self.pump();
    }

    fn update_inspector(&self, window: &ManagerWindow) {
        let selected = window.get_selected_case_id();
        let cover = self.indices.borrow().get(selected.as_str())
            .and_then(|row| self.rows.row_data(*row)).map(|card| card.cover).unwrap_or_default();
        window.set_selected_cover(cover);
    }
}

impl Model for CoverModel {
    type Data = GameCard;
    fn row_count(&self) -> usize { self.rows.row_count() }
    fn row_data(&self, row: usize) -> Option<GameCard> { self.rows.row_data(row) }
    fn model_tracker(&self) -> &dyn ModelTracker { self.rows.model_tracker() }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

impl Drop for CoverModel {
    fn drop(&mut self) {
        self.shared.revision.fetch_add(1, Ordering::AcqRel);
        {
            let mut queue = self.shared.queue.lock();
            queue.stopped = true;
            queue.pending.clear();
        }
        self.shared.wake.notify_one();
        if self.worker.take().is_some_and(|worker| worker.join().is_err()) {
            tracing::error!(target: "astra.emu.cover", code = "worker_panicked", "Cover decoder shutdown failed");
        }
    }
}

fn stamp(path: &Path) -> Result<Option<Stamp>, &'static str> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(Stamp {
            path: path.to_owned(), length: metadata.len(),
            modified: metadata.modified().map_err(|_| "cover_timestamp")?,
        })),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("cover_metadata"),
    }
}

fn resolve(locator: &str) -> Result<Option<Stamp>, &'static str> {
    let path = Path::new(locator);
    if let Some(stamp) = stamp(path)? { return Ok(Some(stamp)); }
    if path.extension().is_none() {
        for extension in ["png", "jpg", "webp", "gif", "bmp"] {
            if let Some(stamp) = stamp(&path.with_extension(extension))? { return Ok(Some(stamp)); }
        }
    }
    Ok(None)
}

fn decode(request: &Request) -> Decoded {
    let before = match resolve(&request.locator) {
        Ok(Some(stamp)) => stamp,
        Ok(None) => return Decoded::Missing,
        Err(code) => return Decoded::Failed(code),
    };
    if request.previous.as_ref() == Some(&before) {
        return Decoded::Unchanged(before);
    }
    let result = (|| {
        let mut reader = image::ImageReader::open(&before.path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound { "cover_disappeared" } else { "cover_open" }
        })?.with_guessed_format().map_err(|_| "cover_format")?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16_384);
        limits.max_image_height = Some(16_384);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().map_err(|_| "cover_decode")?;
        let rgba = if image.width() <= 384 && image.height() <= 512 {
            image.into_rgba8()
        } else {
            image.thumbnail(384, 512).into_rgba8()
        };
        if resolve(&request.locator)?.as_ref() != Some(&before) {
            return Err("cover_changed_during_decode");
        }
        Ok(Pixels { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
    })();
    match result {
        Ok(pixels) => Decoded::Image(before, pixels),
        Err("cover_disappeared") => Decoded::Missing,
        Err(code) => Decoded::Failed(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(row: usize, revision: u64) -> Request {
        Request { revision, row, case_id: format!("game-{row}"), locator: format!("cover-{row}"), previous: None }
    }

    #[test]
    fn reordered_subscriptions_do_not_cancel_another_visible_game() {
        let mut demand = Demand::default();
        demand.record_visible(0, "a", true, "grid");
        demand.record_visible(1, "b", true, "grid");
        demand.record_visible(0, "a", false, "grid");
        demand.record_visible(0, "b", true, "grid");
        demand.record_visible(1, "b", false, "grid");
        demand.record_visible(1, "a", true, "grid");
        assert_eq!(demand.visible["grid"][&0], "b");
        assert_eq!(demand.visible["grid"][&1], "a");
        // A late old subscription cannot erase the replacement in this slot.
        demand.record_visible(0, "a", false, "grid");
        assert_eq!(demand.visible["grid"][&0], "b");
        demand.record_visible(0, "b", false, "grid");
        assert!(!demand.visible["grid"].contains_key(&0));
    }

    #[test]
    fn locator_decodes_real_pixels_and_invalidates_replaced_file() {
        let directory = tempfile::tempdir().unwrap();
        let locator = directory.path().join("game-0");
        let png = locator.with_extension("png");
        image::RgbaImage::from_pixel(2, 3, image::Rgba([12, 34, 56, 255])).save(&png).unwrap();
        let mut request = request(0, 1);
        request.locator = locator.to_str().unwrap().to_owned();
        let Decoded::Image(stamp, pixels) = decode(&request) else { panic!("Expected decoded PNG") };
        assert_eq!((pixels.width, pixels.height), (2, 3));
        assert_eq!(&pixels.rgba[..4], &[12, 34, 56, 255]);
        request.previous = Some(stamp);
        assert!(matches!(decode(&request), Decoded::Unchanged(_)));
        std::fs::remove_file(png).unwrap();
        let bmp = locator.with_extension("bmp");
        image::RgbaImage::from_pixel(4, 5, image::Rgba([98, 76, 54, 255])).save(&bmp).unwrap();
        let Decoded::Image(_, pixels) = decode(&request) else { panic!("Expected replacement BMP") };
        assert_eq!((pixels.width, pixels.height), (4, 5));
        assert_eq!(&pixels.rgba[..4], &[98, 76, 54, 255]);
        std::fs::remove_file(bmp).unwrap();
        assert!(matches!(decode(&request), Decoded::Missing));
    }

    #[test]
    fn saturation_keeps_remaining_visible_work_until_completion() {
        let mut demand = Demand { desired: (0..19).collect(), ..Demand::default() };
        let mut visited = Vec::new();
        while demand.completed.len() != demand.desired.len() {
            let batch = demand.next_rows().collect::<Vec<_>>();
            assert!(!batch.is_empty());
            for row in batch { demand.in_flight.insert(row); }
            assert!(demand.in_flight.len() <= IN_FLIGHT);
            let row = *demand.in_flight.iter().next().unwrap();
            demand.in_flight.remove(&row);
            demand.completed.insert(row);
            visited.push(row);
        }
        visited.sort_unstable();
        assert_eq!(visited, (0..19).collect::<Vec<_>>());
    }

    #[test]
    fn late_completion_cannot_replace_reordered_removed_or_revised_card() {
        let request = request(0, 3);
        let mut demand = Demand { revision: 3, desired: [0].into(), ..Demand::default() };
        let mut card = GameCard { case_id: "game-0".into(), cover_uri: "cover-0".into(), ..GameCard::default() };
        assert!(demand.accepts(&request, Some(&card)));
        assert!(!demand.accepts(&request, None));
        card.case_id = "game-1".into();
        assert!(!demand.accepts(&request, Some(&card)));
        card.case_id = "game-0".into();
        card.cover_uri = "replacement".into();
        assert!(!demand.accepts(&request, Some(&card)));
        card.cover_uri = "cover-0".into();
        demand.revision += 1;
        assert!(!demand.accepts(&request, Some(&card)));
        demand.revision = 3;
        demand.desired.clear();
        assert!(!demand.accepts(&request, Some(&card)));
    }

    #[test]
    fn cancellation_removes_queued_work_and_stop_wakes_waiting_worker() {
        let shared = Arc::new(Shared { queue: Mutex::default(), wake: Condvar::new(), revision: AtomicU64::new(1) });
        shared.push(request(0, 1));
        shared.cancel(2);
        assert!(!shared.current(&request(0, 1)));
        assert!(shared.queue.lock().pending.is_empty());
        let worker_shared = shared.clone();
        let worker = std::thread::spawn(move || worker_shared.next());
        shared.queue.lock().stopped = true;
        shared.wake.notify_one();
        assert!(worker.join().unwrap().is_none());
    }
}
