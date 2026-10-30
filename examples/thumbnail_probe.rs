//! Debug-build regression benchmark for PowerPoint-sized raster pages.
use std::{hint::black_box, time::Instant};
fn main() {
    let pixels = image::RgbaImage::from_fn(4000, 2250, |x, y| {
        image::Rgba([x as u8, y as u8, (x ^ y) as u8, 255])
    });
    let now = Instant::now();
    let reference = image::imageops::thumbnail(&pixels, 320, 180);
    let old = now.elapsed();
    let now = Instant::now();
    let image = presenter::pdf::SlideImage::new(pixels);
    let new = now.elapsed();
    assert_eq!(image.thumbnail().as_raw(), reference.as_raw());
    black_box(image);
    println!(
        "same pixels; generic thumbnail: {old:?}; complete new slide asset (thumbnail + disk backing): {new:?}"
    );
}
