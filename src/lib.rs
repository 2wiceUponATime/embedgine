#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

use core::{
    marker::PhantomData,
    ops::{Index, IndexMut, Range},
};

#[cfg(feature = "embedded-graphics")]
use embedded_graphics::draw_target::DrawTarget;
pub use embedgine_formats as formats;
use formats::{Alpha, AlphaKind, Pixel, Span};

use crate::geometry::URect;
#[cfg(feature = "embedded-graphics")]
use crate::geometry::Vec2;
use crate::geometry::W16;

pub mod geometry;

#[macro_export]
macro_rules! include_assets {
    () => {
        include!(concat!(env!("OUT_DIR"), "/embedgine-build/assets.rs"));
    };
}

#[derive(Debug)]
pub struct Image<'a, Px, K> {
    width: u16,
    pixels: &'a [Px],
    alphas: &'a [u8],
    spans: &'a [Span],
    row_starts: &'a [u16],
    _k: PhantomData<fn() -> K>,
}

impl<'a, Px, K: AlphaKind> Image<'a, Px, K> {
    pub const fn new(
        width: u16,
        pixels: &'a [Px],
        alphas: &'a [u8],
        spans: &'a [Span],
        row_starts: &'a [u16],
    ) -> Self {
        Self {
            width,
            pixels,
            alphas,
            spans,
            row_starts,
            _k: PhantomData,
        }
    }

    pub fn pixels(&self) -> &[Px] {
        self.pixels
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        if K::KIND == Alpha::Opaque {
            self.spans.len() as u16
        } else {
            self.row_starts.len() as u16
        }
    }
}

pub trait FrameSink {
    type Px: Pixel;
    type Error;

    fn present<B: AsRef<[Self::Px]>>(
        &mut self,
        size: Vec2<u16>,
        region: URect<W16>,
        canvas: &Canvas<Self::Px, B>,
    ) -> impl Future<Output = Result<(), Self::Error>>;
}

pub trait Draw<Px, B> {
    fn draw(&self, canvas: &mut Canvas<Px, B>);
}

impl<Px, B, T: Fn(&mut Canvas<Px, B>)> Draw<Px, B> for T {
    fn draw(&self, canvas: &mut Canvas<Px, B>) {
        self(canvas)
    }
}

#[cfg(feature = "embedded-graphics")]
pub struct DrawTargetSink<Px, T> {
    pub target: T,
    _px: PhantomData<Px>,
}

#[cfg(feature = "embedded-graphics")]
impl<Px, T> DrawTargetSink<Px, T> {
    pub fn new(target: T) -> Self {
        Self {
            target,
            _px: PhantomData,
        }
    }
}

#[cfg(feature = "embedded-graphics")]
impl<Px: Pixel, T: DrawTarget<Color: From<Px>>> FrameSink for DrawTargetSink<Px, T> {
    type Px = Px;
    type Error = T::Error;

    async fn present<B: AsRef<[Self::Px]>>(
        &mut self,
        size: Vec2<u16>,
        region: URect<W16>,
        canvas: &Canvas<Self::Px, B>,
    ) -> Result<(), Self::Error> {
        assert_eq!(self.target.bounding_box().size, size.cast().into());
        let colors = (region.top()..region.bottom()).flat_map(|y| {
            (region.left()..region.right()).map(move |x| canvas[Vec2::new(x, y)].into())
        });
        self.target.fill_contiguous(&region.cast().into(), colors)
    }
}

pub struct Renderer<Px, B> {
    size: Vec2<u16>,
    band_height: u16,
    buffers: [Canvas<Px, B>; 2],
    _px: PhantomData<Px>,
}

impl<Px, B> Renderer<Px, B> {
    pub fn new(size: Vec2<u16>, band_height: u16, buffers: [B; 2]) -> Self
    where
        B: AsRef<[Px]>,
    {
        let buffer_len = size.x as usize * band_height as usize;
        for b in &buffers {
            assert_eq!(b.as_ref().len(), buffer_len, "invalid buffer size");
        }
        Self {
            size,
            band_height,
            buffers: buffers.map(|b| Canvas::new(size, 0..band_height, b)),
            _px: PhantomData,
        }
    }

    pub fn width(&self) -> u16 {
        self.size.x
    }

    pub fn height(&self) -> u16 {
        self.size.y
    }

    pub fn size(&self) -> Vec2<u16> {
        self.size
    }

    fn buffer_len(&self) -> usize {
        self.width() as usize * self.height() as usize
    }

    pub fn bands(height: u16, band_height: u16) -> impl Iterator<Item = Range<u16>> {
        (0..height)
            .step_by(band_height as usize)
            .map(move |y| y..(y + band_height).min(height))
    }

    pub fn resize(&mut self, size: Vec2<u16>, band_height: u16, buffers: [B; 2])
    where
        B: AsRef<[Px]>,
    {
        self.size = size;
        self.band_height = band_height;
        for b in &buffers {
            assert_eq!(b.as_ref().len(), self.buffer_len(), "invalid buffer size");
        }
        self.buffers = buffers.map(|b| Canvas::new(size, 0..band_height, b));
    }

    pub fn into_inner(self) -> [Canvas<Px, B>; 2] {
        self.buffers
    }

    pub async fn render<S: FrameSink<Px = Px>>(
        &mut self,
        draw: impl Draw<Px, B>,
        sink: &mut S,
    ) -> Result<(), S::Error>
    where
        B: AsRef<[Px]>,
    {
        let size = self.size;
        let mut canvas_idx = 0;
        let [mut drawing, mut presenting] = self.buffers.each_mut();
        let mut pending = None;
        for band in Self::bands(size.y, self.band_height) {
            drawing.band = band.clone();
            draw.draw(drawing);
            if let Some(f) = { pending } {
                f.await?;
            }
            core::mem::swap(&mut drawing, &mut presenting);
            pending = Some(sink.present(
                size,
                URect::new(
                    Vec2::new(0, band.start),
                    Vec2::new(size.x, band.len() as u16),
                ),
                presenting,
            ));
            canvas_idx = (canvas_idx + 1) % 2;
        }
        if let Some(f) = pending {
            f.await?;
        }
        Ok(())
    }
}

#[cfg(feature = "alloc")]
impl<Px: Clone + Default> Renderer<Px, alloc::vec::Vec<Px>> {
    fn buffers(width: u16, band_height: u16) -> [alloc::vec::Vec<Px>; 2] {
        let buffer_len = width as usize * band_height as usize;
        [
            alloc::vec![Px::default(); buffer_len],
            alloc::vec![Px::default(); buffer_len],
        ]
    }

    pub fn with_size(size: Vec2<u16>, band_height: u16) -> Self {
        Self::new(size, band_height, Self::buffers(size.x, band_height))
    }

    pub fn resize_alloc(&mut self, size: Vec2<u16>, band_height: u16) {
        self.resize(size, band_height, Self::buffers(size.x, band_height));
    }
}

fn map_range<T, U>(range: &Range<T>, mut f: impl FnMut(&T) -> U) -> Range<U> {
    f(&range.start)..f(&range.end)
}

fn intersect_ranges<T: Ord>(a: Range<T>, b: Range<T>) -> Option<Range<T>> {
    let start = a.start.max(b.start);
    let end = a.end.min(b.end);

    if start < end { Some(start..end) } else { None }
}

pub struct Canvas<Px, B> {
    size: Vec2<u16>,
    band: Range<u16>,
    buffer: B,
    _px: PhantomData<Px>,
}

impl<Px, B> Canvas<Px, B> {
    pub fn new(size: Vec2<u16>, band: Range<u16>, buffer: B) -> Self {
        Self {
            size,
            band,
            buffer,
            _px: PhantomData,
        }
    }

    pub fn width(&self) -> u16 {
        self.size.x
    }

    pub fn height(&self) -> u16 {
        self.size.y
    }

    pub fn size(&self) -> Vec2<u16> {
        self.size
    }

    pub fn full(size: Vec2<u16>, buffer: B) -> Self {
        Self::new(size, 0..size.y, buffer)
    }

    pub fn band(&self) -> &Range<u16> {
        &self.band
    }

    fn buffer_index(&self, Vec2 { x, y }: Vec2<u16>) -> usize {
        (y - self.band.start) as usize * self.size.x as usize + x as usize
    }
}

impl<Px: Copy, B: AsMut<[Px]>> Canvas<Px, B> {
    pub fn fill(&mut self, color: Px) {
        self.buffer.as_mut().fill(color);
    }

    pub fn draw_image<K: AlphaKind>(&mut self, image: &Image<'_, Px, K>, pos: Vec2<i16>) {
        match K::KIND {
            Alpha::Opaque => {
                let Some(x_range) =
                    intersect_ranges(0..self.width() as i16, pos.x..pos.x + image.width as i16)
                else {
                    return;
                };
                let Some(y_range) = intersect_ranges(
                    map_range(&self.band, |v| *v as i16),
                    pos.y..pos.y + image.height() as i16,
                ) else {
                    return;
                };
                let x_range = map_range(&x_range, |&v| v as u16);
                let y_range = map_range(&y_range, |&v| v as u16);
                let rel_x_range = map_range(&x_range, |&v| (v as i16 - pos.x) as usize);
                let rel_y_range = map_range(&y_range, |&v| (v as i16 - pos.y) as usize);
                let mut i = image.width as u32 * rel_y_range.start as u32;
                for (y, span) in image.spans[rel_y_range.start..].iter().enumerate() {
                    let y = y_range.start + y as u16;
                    if y >= y_range.end {
                        break;
                    }
                    let start = i;
                    i += span.len as u32;
                    let row = &image.pixels[start as usize..i as usize][rel_x_range.clone()];
                    let buffer_index = self.buffer_index(Vec2::new(x_range.start, y));
                    self.buffer.as_mut()[buffer_index..buffer_index + x_range.len()]
                        .copy_from_slice(row);
                }
            }
            Alpha::Masked => todo!(),
            Alpha::Blended => todo!(),
        }
    }
}

impl<Px, B: AsRef<[Px]>> Index<Vec2<u16>> for Canvas<Px, B> {
    type Output = Px;

    fn index(&self, pos: Vec2<u16>) -> &Self::Output {
        &self.buffer.as_ref()[self.buffer_index(pos)]
    }
}

impl<Px, B: AsRef<[Px]> + AsMut<[Px]>> IndexMut<Vec2<u16>> for Canvas<Px, B> {
    fn index_mut(&mut self, pos: Vec2<u16>) -> &mut Self::Output {
        let i = self.buffer_index(pos);
        &mut self.buffer.as_mut()[i]
    }
}
