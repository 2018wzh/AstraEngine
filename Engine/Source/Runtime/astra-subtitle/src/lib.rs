//! Native ASS rendering at an explicit media timestamp, with no engine/VM/SDK
//! dependency. Create, render and drop on the owning thread.
//!
//! Only supplied memory fonts are visible. Embedded ASS attachments, system
//! providers, default paths and default families are disabled. Font/glyph
//! failures and native warnings fail closed; a failed native renderer remains
//! failed because libass can cache incomplete shaping results. Font availability
//! is checked by libass when an event is rendered, including inline font tags.
//!
//! `SubtitleFrame::matrix` must be combined with actual video metadata (see
//! `YcbcrMatrix::rgb_for_video`); raster dimensions are not colorspace metadata.
//! `raster` is the complete display raster, with no letterbox/crop margins.
//!
//! ```compile_fail
//! # use astra_subtitle::AssRenderer;
//! # fn example(renderer: &mut AssRenderer) {
//! let old = renderer.render(0, (640, 360), (640, 360)).unwrap();
//! renderer.render(1000, (640, 360), (640, 360)).unwrap();
//! let _ = old.runs.len(); // a frame cannot survive the next render
//! # }
//! ```
//!
//! ```compile_fail
//! # use astra_subtitle::AssRenderer;
//! # fn example(renderer: AssRenderer) {
//! std::thread::spawn(move || drop(renderer)); // owner-thread handle is !Send
//! # }
//! ```
mod color;
mod ffi;

pub use color::{VideoColorSpace, YcbcrMatrix};
use std::{
    ffi::CString,
    fmt,
    marker::PhantomData,
    ptr::{self, NonNull},
    rc::Rc,
    sync::Arc,
};

#[derive(Clone)]
pub struct FontData {
    /// Attachment label, not a substitute for the font's internal family name.
    pub name: String,
    pub bytes: Arc<[u8]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    Initialize,
    Fonts,
    Parse,
    Render,
    Color,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    Version,
    Input,
    Native,
    MissingFont,
    MissingGlyph,
    Bounds,
    Memory,
    ColorSpace,
}

/// Contains only a safe category and stage, never native diagnostic text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubtitleError {
    pub kind: ErrorKind,
    pub stage: Stage,
}
impl SubtitleError {
    pub(crate) const fn new(kind: ErrorKind, stage: Stage) -> Self {
        Self { kind, stage }
    }
}
impl fmt::Display for SubtitleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "subtitle {:?} at {:?}", self.kind, self.stage)
    }
}
impl std::error::Error for SubtitleError {}

/// Preserves libass's 0/1/2 distinction. Position-only changes do not require
/// uploading masks again; both positions and colors belong to ordered runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameChange {
    Unchanged,
    Positions,
    Content,
}

pub struct SubtitleFrame<'a> {
    pub changed: bool,
    pub change: FrameChange,
    pub matrix: YcbcrMatrix,
    pub runs: BitmapRuns<'a>,
}

#[derive(Clone, Copy, Debug)]
pub struct BitmapRun<'a> {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub stride: usize,
    /// Native 0xRRGGBBAA, with inverted ASS alpha (0 is opaque).
    pub color: u32,
    /// Exact `stride * (h - 1) + w` bytes; padding is initialized but not
    /// coverage. Consume only `w` bytes of each row. Borrowed until next render.
    pub alpha: &'a [u8],
}
impl BitmapRun<'_> {
    pub fn opacity(&self) -> u8 {
        255 - (self.color as u8)
    }
    /// Effective 8-bit straight alpha at a valid mask pixel, rounded to nearest.
    pub fn pixel_alpha(&self, coverage: u8) -> u8 {
        ((u16::from(coverage) * u16::from(self.opacity()) + 127) / 255) as u8
    }
    /// Straight RGBA for actual SDR video metadata; mask coverage is separate.
    pub fn rgba_for_video(
        &self,
        matrix: YcbcrMatrix,
        video: VideoColorSpace,
    ) -> Result<[u8; 4], SubtitleError> {
        let rgb = matrix.rgb_for_video(
            [
                (self.color >> 24) as u8,
                (self.color >> 16) as u8,
                (self.color >> 8) as u8,
            ],
            video,
        )?;
        Ok([rgb[0], rgb[1], rgb[2], self.opacity()])
    }
}

#[derive(Clone, Copy)]
struct RunDescriptor {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    stride: usize,
    color: u32,
    source: *const u8,
    len: usize,
    mask_offset: Option<usize>,
}

/// A borrowed ordered view over reused descriptors, with no per-frame Vec.
#[derive(Clone, Copy)]
pub struct BitmapRuns<'a> {
    descriptors: &'a [RunDescriptor],
    masks: &'a [u8],
}
impl<'a> BitmapRuns<'a> {
    pub fn len(&self) -> usize {
        self.descriptors.len()
    }
    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = BitmapRun<'a>> + 'a {
        let descriptors = self.descriptors;
        let masks = self.masks;
        descriptors.iter().map(move |run| {
            let alpha = match run.mask_offset {
                Some(offset) => &masks[offset..offset + run.len],
                // SAFETY: these are tightly packed native masks validated in
                // render; the exclusive renderer borrow pins their lifetime.
                None => unsafe { std::slice::from_raw_parts(run.source, run.len) },
            };
            BitmapRun {
                x: run.x,
                y: run.y,
                w: run.w,
                h: run.h,
                stride: run.stride,
                color: run.color,
                alpha,
            }
        })
    }
}

/// Not Send or Sync: the native objects and callback belong to this owner thread.
/// Drop always releases track, then renderer, then library, then callback state.
pub struct AssRenderer {
    track: Option<NonNull<ffi::Track>>,
    renderer: Option<NonNull<ffi::Renderer>>,
    library: NonNull<ffi::Library>,
    status: Box<i32>,
    matrix: YcbcrMatrix,
    runs: Vec<RunDescriptor>,
    masks: Vec<u8>,
    masks_valid: bool,
    _owner_thread: PhantomData<Rc<()>>,
}

impl AssRenderer {
    pub fn new(ass_utf8: &[u8], fonts: &[FontData]) -> Result<Self, SubtitleError> {
        if ass_utf8.is_empty() || ass_utf8.contains(&0) || std::str::from_utf8(ass_utf8).is_err() {
            return Err(SubtitleError::new(ErrorKind::Input, Stage::Parse));
        }
        // SAFETY: version query has no handle or ownership requirements.
        if unsafe { ffi::ass_library_version() } != ffi::VERSION {
            return Err(SubtitleError::new(ErrorKind::Version, Stage::Initialize));
        }
        // SAFETY: the library is newly allocated and owned exclusively here.
        let library = NonNull::new(unsafe { ffi::ass_library_init() })
            .ok_or(SubtitleError::new(ErrorKind::Native, Stage::Initialize))?;
        let mut result = Self {
            track: None,
            renderer: None,
            library,
            status: Box::new(0),
            matrix: YcbcrMatrix::Default,
            runs: Vec::new(),
            masks: Vec::new(),
            masks_valid: false,
            _owner_thread: PhantomData,
        };
        // SAFETY: callback state has a stable address and outlives all handles.
        unsafe { ffi::astra_ass_quiet(library.as_ptr(), &mut *result.status) };
        for font in fonts {
            let name = CString::new(font.name.as_str())
                .map_err(|_| SubtitleError::new(ErrorKind::Input, Stage::Fonts))?;
            let size = i32::try_from(font.bytes.len())
                .map_err(|_| SubtitleError::new(ErrorKind::Bounds, Stage::Fonts))?;
            if size == 0 || font.name.is_empty() {
                return Err(SubtitleError::new(ErrorKind::Input, Stage::Fonts));
            }
            // SAFETY: libass copies both the label and font bytes synchronously.
            unsafe {
                ffi::ass_add_font(
                    library.as_ptr(),
                    name.as_ptr(),
                    font.bytes.as_ptr().cast(),
                    size,
                )
            };
        }
        // SAFETY: render handle belongs to the still-live library.
        result.renderer = NonNull::new(unsafe { ffi::ass_renderer_init(library.as_ptr()) });
        let renderer = result
            .renderer
            .ok_or(SubtitleError::new(ErrorKind::Native, Stage::Initialize))?;
        result.check_status(Stage::Initialize)?;
        // No default path/family means there is no fallback even among supplied
        // fonts of a different family. NONE also disables OS font providers.
        unsafe {
            ffi::ass_set_fonts(
                renderer.as_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                ptr::null(),
                0,
            )
        };
        result.check_status(Stage::Fonts)?;
        // ass_read_memory's pinned implementation copies its input before
        // parsing; it does not modify our shared slice (its public ABI is char*).
        result.track = NonNull::new(unsafe {
            ffi::ass_read_memory(
                library.as_ptr(),
                ass_utf8.as_ptr().cast_mut().cast(),
                ass_utf8.len(),
                ptr::null(),
            )
        });
        let track = result
            .track
            .ok_or(SubtitleError::new(ErrorKind::Input, Stage::Parse))?;
        result.check_status(Stage::Parse)?;
        // Read-only header access in C uses the actual pinned official layout.
        if unsafe { ffi::astra_ass_track_valid(track.as_ptr()) } == 0 {
            return Err(SubtitleError::new(ErrorKind::Input, Stage::Parse));
        }
        result.matrix = YcbcrMatrix::from_native(unsafe { ffi::astra_ass_matrix(track.as_ptr()) })?;
        Ok(result)
    }

    fn check_status(&self, stage: Stage) -> Result<(), SubtitleError> {
        let kind = match *self.status {
            0 => return Ok(()),
            2 => ErrorKind::MissingFont,
            3 => ErrorKind::MissingGlyph,
            _ => ErrorKind::Native,
        };
        Err(SubtitleError::new(kind, stage))
    }

    pub fn render(
        &mut self,
        position_ms: u64,
        storage: (u32, u32),
        raster: (u32, u32),
    ) -> Result<SubtitleFrame<'_>, SubtitleError> {
        self.check_status(Stage::Render)?;
        let now = i64::try_from(position_ms)
            .map_err(|_| SubtitleError::new(ErrorKind::Bounds, Stage::Render))?;
        let [sw, sh, rw, rh] = [storage.0, storage.1, raster.0, raster.1].map(i32::try_from);
        let dimensions = || -> Option<(i32, i32, i32, i32)> {
            let (sw, sh, rw, rh) = (sw.ok()?, sh.ok()?, rw.ok()?, rh.ok()?);
            (sw > 0
                && sh > 0
                && rw > 0
                && rh > 0
                && sw.checked_mul(sh).is_some()
                && rw.checked_mul(rh).is_some())
            .then_some((sw, sh, rw, rh))
        };
        let (sw, sh, rw, rh) =
            dimensions().ok_or(SubtitleError::new(ErrorKind::Bounds, Stage::Render))?;
        let renderer = self.renderer.expect("initialized renderer").as_ptr();
        let track = self.track.expect("initialized track").as_ptr();
        let mut native_change = 0;
        let reuse_masks = self.masks_valid;
        self.masks_valid = false;
        // SAFETY: no prior frame borrow can coexist with this exclusive borrow.
        // No Rust reference into native memory is kept in descriptor storage.
        let mut image = unsafe {
            ffi::ass_set_storage_size(renderer, sw, sh);
            ffi::ass_set_frame_size(renderer, rw, rh);
            ffi::ass_render_frame(renderer, track, now, &mut native_change)
        };
        self.check_status(Stage::Render)?;
        let change = match native_change {
            0 => FrameChange::Unchanged,
            1 => FrameChange::Positions,
            2 => FrameChange::Content,
            _ => return Err(SubtitleError::new(ErrorKind::Native, Stage::Render)),
        };
        let mut count = 0;
        let mut mask_end = 0usize;
        while let Some(current) = NonNull::new(image) {
            // SAFETY: official image list is live until next native render/drop.
            let native = unsafe { current.as_ref() };
            image = native.next;
            let len = bitmap_len(native.w, native.h, native.stride)?;
            if len == 0 {
                continue;
            }
            let (x, y) = (u32::try_from(native.dst_x), u32::try_from(native.dst_y));
            let (x, y) = match (x, y) {
                (Ok(x), Ok(y))
                    if x.checked_add(native.w as u32)
                        .is_some_and(|end| end <= raster.0)
                        && y.checked_add(native.h as u32)
                            .is_some_and(|end| end <= raster.1) =>
                {
                    (x, y)
                }
                _ => return Err(SubtitleError::new(ErrorKind::Bounds, Stage::Render)),
            };
            if native.bitmap.is_null() {
                return Err(SubtitleError::new(ErrorKind::Native, Stage::Render));
            }
            let padded = native.h > 1 && native.stride > native.w;
            let old = self.runs.get(count).copied();
            let offset = if padded {
                let start = mask_end;
                mask_end = start
                    .checked_add(len)
                    .filter(|&end| end <= isize::MAX as usize)
                    .ok_or(SubtitleError::new(ErrorKind::Bounds, Stage::Render))?;
                if self.masks.len() < mask_end {
                    self.masks
                        .try_reserve(mask_end - self.masks.len())
                        .map_err(|_| SubtitleError::new(ErrorKind::Memory, Stage::Render))?;
                    self.masks.resize(mask_end, 0);
                }
                let reusable = reuse_masks
                    && change != FrameChange::Content
                    && old.is_some_and(|run| {
                        run.w == native.w as u32
                            && run.h == native.h as u32
                            && run.stride == native.stride as usize
                            && run.mask_offset == Some(start)
                            && run.len == len
                    });
                if !reusable {
                    // Native padding may be uninitialized. Only read initialized
                    // width bytes; renderer-owned padding remains initialized.
                    for row in 0..native.h as usize {
                        let at = row * native.stride as usize;
                        unsafe {
                            ptr::copy_nonoverlapping(
                                native.bitmap.add(at),
                                self.masks.as_mut_ptr().add(start + at),
                                native.w as usize,
                            );
                        }
                    }
                }
                Some(start)
            } else {
                None
            };
            let run = RunDescriptor {
                x,
                y,
                w: native.w as u32,
                h: native.h as u32,
                stride: native.stride as usize,
                color: native.color,
                source: native.bitmap,
                len,
                mask_offset: offset,
            };
            if count < self.runs.len() {
                self.runs[count] = run;
            } else {
                self.runs
                    .try_reserve(1)
                    .map_err(|_| SubtitleError::new(ErrorKind::Memory, Stage::Render))?;
                self.runs.push(run);
            }
            count += 1;
        }
        self.runs.truncate(count);
        self.masks_valid = true;
        // Keep mask length/capacity: initialized storage is reused after seeks.
        Ok(SubtitleFrame {
            changed: change != FrameChange::Unchanged,
            change,
            matrix: self.matrix,
            runs: BitmapRuns {
                descriptors: &self.runs,
                masks: &self.masks,
            },
        })
    }
}

fn bitmap_len(w: i32, h: i32, stride: i32) -> Result<usize, SubtitleError> {
    if w < 0 || h < 0 || stride < 0 {
        return Err(SubtitleError::new(ErrorKind::Bounds, Stage::Render));
    }
    if w == 0 || h == 0 {
        return Ok(0);
    }
    if stride < w {
        return Err(SubtitleError::new(ErrorKind::Bounds, Stage::Render));
    }
    (stride as usize)
        .checked_mul(h as usize - 1)
        .and_then(|n| n.checked_add(w as usize))
        .filter(|&n| n <= isize::MAX as usize)
        .ok_or(SubtitleError::new(ErrorKind::Bounds, Stage::Render))
}

impl Drop for AssRenderer {
    fn drop(&mut self) {
        // SAFETY: exclusive owner; callback status remains alive through all
        // finalizers. Partial construction follows the same dependency order.
        unsafe {
            if let Some(track) = self.track.take() {
                ffi::ass_free_track(track.as_ptr());
            }
            if let Some(renderer) = self.renderer.take() {
                ffi::ass_renderer_done(renderer.as_ptr());
            }
            ffi::ass_library_done(self.library.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_row_is_unpadded_and_lengths_are_checked() {
        assert_eq!(bitmap_len(3, 2, 8), Ok(11));
        assert_eq!(bitmap_len(3, 1, 8), Ok(3));
        assert_eq!(bitmap_len(0, 2, 8), Ok(0));
        assert_eq!(bitmap_len(3, 0, 8), Ok(0));
        assert_eq!(bitmap_len(3, 0, 0), Ok(0));
        for dimensions in [(-1, 2, 8), (3, -1, 8), (3, 2, -1), (9, 2, 8)] {
            assert!(bitmap_len(dimensions.0, dimensions.1, dimensions.2).is_err());
        }
        let exact = [255u8, 128, 64, 0, 0, 0, 0, 0, 32, 16, 8];
        let descriptors = [RunDescriptor {
            x: 0,
            y: 0,
            w: 3,
            h: 2,
            stride: 8,
            color: 0x12345680,
            source: ptr::null(),
            len: 11,
            mask_offset: Some(0),
        }];
        let runs = BitmapRuns {
            descriptors: &descriptors,
            masks: &exact,
        };
        let run = runs.iter().next().unwrap();
        assert_eq!(&run.alpha[8..], &[32, 16, 8]);
        assert_eq!(run.opacity(), 127);
        assert_eq!(run.pixel_alpha(255), 127);
        assert_eq!(run.pixel_alpha(128), 64);
        assert_eq!(
            run.rgba_for_video(YcbcrMatrix::None, VideoColorSpace::Rgb),
            Ok([0x12, 0x34, 0x56, 127])
        );
    }
}
