#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

use core::marker::PhantomData;

pub use embedgine_formats as formats;
use formats::{Pixel, Span};

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

impl<'a, Px, K> Image<'a, Px, K> {
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
}

pub struct Renderer<Px, B> {
    width: u32,
    height: u32,
    band_height: u32,
    buffers: [B; 2],
    _px: PhantomData<Px>,
}

fn area(width: u32, height: u32) -> usize {
    width as usize * height as usize
}

impl<Px, B> Renderer<Px, B> {
    pub fn new(width: u32, height: u32, band_height: u32, buffers: [B; 2]) -> Self
    where
        B: AsRef<[Px]>,
    {
        let buffer_len = area(width, band_height);
        for b in &buffers {
            assert_eq!(b.as_ref().len(), buffer_len, "invalid buffer size");
        }
        Self {
            width,
            height,
            band_height,
            buffers,
            _px: PhantomData,
        }
    }

    fn buffer_len(&self) -> usize {
        area(self.width, self.height)
    }
}

impl<Px, B: AsRef<[Px]> + AsMut<[Px]>> Renderer<Px, B> {
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn resize(&mut self, width: u32, height: u32, band_height: u32, buffers: [B; 2]) {
        self.width = width;
        self.height = height;
        self.band_height = band_height;
        self.buffers = buffers;
        assert_eq!(
            self.buffer_len(),
            self.buffers.as_ref().len(),
            "invalid buffer size",
        );
    }

    pub fn into_inner(self) -> [B; 2] {
        self.buffers
    }
}

#[cfg(feature = "alloc")]
impl<Px: Clone + Default> Renderer<Px, alloc::vec::Vec<Px>> {
    fn buffers(width: u32, band_height: u32) -> [alloc::vec::Vec<Px>; 2] {
        let buffer_len = area(width, band_height);
        [
            alloc::vec![Px::default(); buffer_len],
            alloc::vec![Px::default(); buffer_len],
        ]
    }

    pub fn with_size(width: u32, height: u32, band_height: u32) -> Self {
        Self::new(width, height, band_height, Self::buffers(width, height))
    }

    pub fn resize_alloc(&mut self, width: u32, height: u32, band_height: u32) {
        self.resize(width, height, band_height, Self::buffers(width, height));
    }
}
