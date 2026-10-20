#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

use core::marker::PhantomData;

pub use embedgine_formats as formats;
use embedgine_formats::Pixel;

#[macro_export]
macro_rules! include_assets {
    () => {
        include!(concat!(env!("OUT_DIR"), "/embedgine-build/assets.rs"));
    };
}

#[derive(Debug)]
pub struct Image<Px, B> {
    width: u32,
    height: u32,
    data: B,
    _px: PhantomData<fn() -> Px>,
}

impl<Px, B> Image<Px, B> {
    pub const fn new(width: u32, height: u32, data: B) -> Self {
        Self {
            width,
            height,
            data,
            _px: PhantomData,
        }
    }
}

impl<Px, B: AsRef<[Px]>> Image<Px, B> {
    pub fn pixels(&self) -> &[Px] {
        self.data.as_ref()
    }
}

impl<Px, B: AsMut<[Px]>> Image<Px, B> {
    pub fn pixels_mut(&mut self) -> &mut [Px] {
        self.data.as_mut()
    }
}

pub trait FrameSink {
    type Px: Pixel;
}

pub struct Renderer<Px, B> {
    width: u32,
    height: u32,
    buffer: B,
    _px: PhantomData<Px>,
}

impl<Px, B: AsRef<[Px]>> Renderer<Px, B> {
    pub fn new(width: u32, height: u32, buffer: B) -> Self {
        assert_eq!(
            width as usize * height as usize,
            buffer.as_ref().len(),
            "invalid buffer size",
        );
        Self {
            width,
            height,
            buffer,
            _px: PhantomData,
        }
    }
}

impl<Px, B: AsRef<[Px]> + AsMut<[Px]>> Renderer<Px, B> {
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn resize(&mut self, width: u32, height: u32, buffer: B) {
        assert_eq!(
            width as usize * height as usize,
            buffer.as_ref().len(),
            "invalid buffer size",
        );
        self.width = width;
        self.height = height;
        self.buffer = buffer;
    }

    pub fn into_inner(self) -> B {
        self.buffer
    }
}

#[cfg(feature = "alloc")]
impl<Px: Clone + Default> Renderer<Px, alloc::vec::Vec<Px>> {
    fn buffer(width: u32, height: u32) -> alloc::vec::Vec<Px> {
        alloc::vec![Px::default(); width as usize * height as usize]
    }

    pub fn with_size(width: u32, height: u32) -> Self {
        Self::new(width, height, Self::buffer(width, height))
    }

    pub fn resize_alloc(&mut self, width: u32, height: u32) {
        self.resize(width, height, Self::buffer(width, height));
    }
}
