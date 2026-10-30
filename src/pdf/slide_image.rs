//! Exact RGBA slide pixels backed by private temporary files. Only a byte-bounded
//! working set is copied into the heap; thumbnails never fault full-size pixels in.
use image::{Rgba, RgbaImage};
use memmap2::Mmap;
use std::{
    collections::HashMap,
    io::Write,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

const PIXEL_BUDGET: usize = 64 * 1024 * 1024;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static CACHE: OnceLock<Mutex<PixelCache>> = OnceLock::new();
#[derive(Default)]
struct PixelCache {
    entries: HashMap<u64, (Arc<RgbaImage>, u64)>,
    bytes: usize,
    clock: u64,
}
fn cache() -> &'static Mutex<PixelCache> {
    CACHE.get_or_init(Default::default)
}

enum Backing {
    Memory(Arc<RgbaImage>),
    File { map: Mmap },
}
pub struct SlideImage {
    id: u64,
    width: u32,
    height: u32,
    backing: Backing,
    thumbnail: OnceLock<RgbaImage>,
}
#[allow(dead_code)] // Some public library helpers are unused by the desktop binary.
impl SlideImage {
    pub fn new(image: RgbaImage) -> Self {
        Self::make(image, true, false)
    }
    /// Notes are never shown in the overview; avoid generating unused thumbnails.
    pub fn new_notes(image: RgbaImage) -> Self {
        Self::make(image, false, false)
    }
    /// The fixed demo deck is small; keep it resident without temporary-file startup work.
    pub fn new_resident(image: RgbaImage) -> Self {
        Self::make(image, false, true)
    }
    fn make(image: RgbaImage, make_thumbnail: bool, resident: bool) -> Self {
        let (width, height) = image.dimensions();
        let target_h = ((320.0 * height as f64 / width.max(1) as f64) as u32).max(1);
        let thumbnail = OnceLock::new();
        let image = if make_thumbnail {
            // DynamicImage's non-generic entry point runs the exact same thumbnail
            // kernel in the optimized image dependency. Calling the generic helper
            // here instantiates it in the unoptimized app during `cargo run`, which
            // makes large PowerPoint pages take seconds per thumbnail.
            let dynamic = image::DynamicImage::ImageRgba8(image);
            let _ = thumbnail.set(dynamic.thumbnail_exact(320, target_h).into_rgba8());
            dynamic.into_rgba8()
        } else {
            image
        };
        let mapped = || -> std::io::Result<Backing> {
            let mut file = tempfile::tempfile()?;
            file.write_all(image.as_raw())?;
            // SAFETY: this private file is never modified or exposed after mapping.
            // The mapping keeps the file object alive after the private handle closes.
            let map = unsafe { Mmap::map(&file)? };
            Ok(Backing::File { map })
        };
        let backing = if !resident && image.as_raw().len() >= 1024 * 1024 {
            mapped().unwrap_or_else(|_| Backing::Memory(Arc::new(image)))
        } else {
            Backing::Memory(Arc::new(image))
        };
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            backing,
            thumbnail,
        }
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn thumbnail(&self) -> &RgbaImage {
        self.thumbnail.get_or_init(|| {
            let target_h = ((320.0 * self.height as f64 / self.width.max(1) as f64) as u32).max(1);
            image::DynamicImage::ImageRgba8((*self.pixels()).clone())
                .thumbnail_exact(320, target_h)
                .into_rgba8()
        })
    }
    pub fn pixels(&self) -> Arc<RgbaImage> {
        let Backing::File { map, .. } = &self.backing else {
            let Backing::Memory(image) = &self.backing else {
                unreachable!()
            };
            return image.clone();
        };
        let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
        c.clock += 1;
        let clock = c.clock;
        if let Some((image, used)) = c.entries.get_mut(&self.id) {
            *used = clock;
            return image.clone();
        }
        let bytes = map.len();
        while c.bytes + bytes > PIXEL_BUDGET && !c.entries.is_empty() {
            let key = *c.entries.iter().min_by_key(|(_, (_, t))| *t).unwrap().0;
            let (image, _) = c.entries.remove(&key).unwrap();
            c.bytes -= image.as_raw().len();
        }
        let image = Arc::new(
            RgbaImage::from_raw(self.width, self.height, map.to_vec())
                .expect("validated RGBA dimensions"),
        );
        #[cfg(unix)]
        {
            // SAFETY: read-only file mapping; discarding resident pages reloads the
            // same immutable file bytes and cannot lose or change slide data.
            let _ = unsafe { map.unchecked_advise(memmap2::UncheckedAdvice::DontNeed) };
        }
        // An exceptionally large page is usable without growing the retained cache.
        if bytes <= PIXEL_BUDGET {
            c.bytes += bytes;
            c.entries.insert(self.id, (image.clone(), clock));
        }
        image
    }
    pub fn get_pixel(&self, x: u32, y: u32) -> Rgba<u8> {
        *self.pixels().get_pixel(x, y)
    }
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> image::ImageResult<()> {
        self.pixels().save(path)
    }
    pub fn cached_bytes() -> usize {
        cache().lock().unwrap_or_else(|e| e.into_inner()).bytes
    }
}
impl Drop for SlideImage {
    fn drop(&mut self) {
        let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((image, _)) = c.entries.remove(&self.id) {
            c.bytes -= image.as_raw().len();
        }
    }
}
