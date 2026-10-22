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
use embedgine_formats::AlphaKind;
use formats::{Pixel, Span};

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
    width: u32,
    height: u32,
    pixels: &'a [Px],
    alphas: &'a [u8],
    spans: &'a [Span],
    _k: PhantomData<fn() -> K>,
}

impl<'a, Px, K: AlphaKind> Image<'a, Px, K> {
    pub const fn new(
        width: u32,
        height: u32,
        pixels: &'a [Px],
        alphas: &'a [u8],
        spans: &'a [Span],
    ) -> Self {
        Self {
            width,
            height,
            pixels,
            alphas,
            spans,
            _k: PhantomData,
        }
    }

    pub fn pixels(&self) -> &[Px] {
        self.pixels
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
        let mut canvas_idx = 0;
        for band in Self::bands(self.height(), self.band_height) {
            let canvas = &mut self.buffers[canvas_idx];
            canvas.band = band.clone();
            draw.draw(canvas);
            sink.present(
                self.size,
                URect::new(
                    Vec2::new(0, band.start),
                    Vec2::new(self.width(), band.len() as u16),
                ),
                &self.buffers[canvas_idx],
            )
            .await?;
            canvas_idx = (canvas_idx + 1) % 2;
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

    pub fn full(size: Vec2<u16>, buffer: B) -> Self {
        Self::new(size, 0..size.y, buffer)
    }

    pub fn band(&self) -> &Range<u16> {
        &self.band
    }

    fn buffer_index(&self, Vec2 { x, y }: Vec2<u16>) -> usize {
        (y - self.band.start) as usize * self.size.x as usize + x as usize
    }

    pub fn fill(&mut self, color: Px)
    where
        Px: Clone,
        B: AsMut<[Px]>,
    {
        self.buffer.as_mut().fill(color);
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
