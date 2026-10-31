//! Slide pixels → GPU-ready `slint::Image`s (cached on the UI thread) and thumbnails.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::pdf::SlideImage;
use image::RgbaImage;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

use crate::pdf::{PresentationDeck, Slide};
use crate::{PresenterWindow, SlideThumbnail};

pub(crate) fn rgba_to_slint_image(img: &RgbaImage) -> Image {
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(img.width(), img.height());
    buffer.make_mut_bytes().copy_from_slice(img.as_raw());
    Image::from_rgba8(buffer)
}

/// GPU-ready `slint::Image`s for slide pixels, kept on the UI thread (slint images aren't
/// `Send`, slides are). Keyed by the slide image's `Arc` allocation; the stored `Weak` pins
/// that allocation so a key can never be reused by a different image, without keeping the
/// pixels alive. Replacing a slide's image (rotate, reload) naturally misses the cache.
#[derive(Default)]
pub(crate) struct GpuImageCache {
    pub(crate) full: HashMap<usize, (std::sync::Weak<SlideImage>, Image)>,
    pub(crate) thumbs: HashMap<usize, (std::sync::Weak<SlideImage>, Image)>,
}

thread_local! {
    static GPU_CACHE: RefCell<GpuImageCache> = RefCell::new(GpuImageCache::default());
}

pub(crate) fn cache_key(img: &Arc<SlideImage>) -> usize {
    Arc::as_ptr(img) as usize
}

pub(crate) fn cached_lookup(
    map: &mut HashMap<usize, (std::sync::Weak<SlideImage>, Image)>,
    img: &Arc<SlideImage>,
    build: impl FnOnce(&SlideImage) -> Image,
) -> Image {
    let key = cache_key(img);
    if let Some((_, i)) = map.get(&key) {
        return i.clone();
    }
    let i = build(img);
    map.insert(key, (Arc::downgrade(img), i.clone()));
    i
}

/// Full-resolution GPU image for slide pixels, converted on first use.
pub(crate) fn cached_slint_image(img: &Arc<SlideImage>) -> Image {
    GPU_CACHE
        .with_borrow_mut(|c| cached_lookup(&mut c.full, img, |i| rgba_to_slint_image(&i.pixels())))
}

/// Drops full-resolution GPU images except those for `keep` (and anything whose slide is gone).
pub(crate) fn evict_gpu_images_except(keep: &[&Arc<SlideImage>]) {
    let keep: Vec<usize> = keep.iter().map(|a| cache_key(a)).collect();
    GPU_CACHE.with_borrow_mut(|c| {
        c.full.retain(|k, _| keep.contains(k));
        c.thumbs.retain(|_, (weak, _)| weak.strong_count() > 0);
    });
}

pub(crate) fn slide_audience_image(s: &Slide) -> Image {
    cached_slint_image(&s.audience_image)
}

/// The presenter view usually shows the same pixels as the audience view; share one GPU image then.
pub(crate) fn slide_presenter_image(s: &Slide) -> Image {
    cached_slint_image(&s.presenter_image)
}

pub(crate) fn slide_thumbnail_image(s: &Slide) -> Image {
    GPU_CACHE.with_borrow_mut(|c| {
        cached_lookup(&mut c.thumbs, &s.audience_image, |img| {
            rgba_to_slint_image(img.thumbnail())
        })
    })
}

/// Shown in the "next slide" preview after the last slide. Built once per thread.
pub(crate) fn end_of_deck_image() -> Image {
    thread_local! {
        static END_IMG: Image = {
            let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(16, 9);
            for px in buffer.make_mut_slice() {
                *px = Rgba8Pixel { r: 15, g: 15, b: 18, a: 255 };
            }
            Image::from_rgba8(buffer)
        };
    }
    END_IMG.with(|i| i.clone())
}

pub(crate) fn update_slide_thumbnails_model(deck: &PresentationDeck, presenter: &PresenterWindow) {
    // Thumbnails are cached per slide, so this only rescales slides that are new or changed.
    let thumbs: Vec<SlideThumbnail> = deck
        .slides
        .iter()
        .enumerate()
        .map(|(i, slide)| SlideThumbnail {
            is_hidden: slide.is_hidden,
            index: (i + 1) as i32,
            image: slide_thumbnail_image(slide),
        })
        .collect();
    thread_local! { static MODEL: RefCell<Option<std::rc::Rc<slint::VecModel<SlideThumbnail>>>> = const { RefCell::new(None) }; }
    use slint::Model;
    MODEL.with_borrow_mut(|stored| {
        let model = stored.get_or_insert_with(|| std::rc::Rc::new(slint::VecModel::default()));
        if model.row_count() != thumbs.len() {
            model.set_vec(thumbs);
        } else {
            for (row, thumb) in thumbs.into_iter().enumerate() {
                if model.row_data(row).as_ref() != Some(&thumb) {
                    model.set_row_data(row, thumb);
                }
            }
        }
        presenter.set_slide_thumbnails(model.clone().into());
    });
}

#[cfg(test)]
pub(crate) fn full_cache_len() -> usize {
    GPU_CACHE.with_borrow(|cache| cache.full.len())
}
