#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

use core::{
    marker::PhantomData,
    ops::{BitAnd, BitOr, BitXor, Index, IndexMut, Not, Range},
};

#[cfg(feature = "embedded-graphics")]
use embedded_graphics_core::draw_target::DrawTarget;
pub use embedgine_formats as formats;
use embedgine_formats::{Blend, Blended, Masked, Opaque, RowStart, SpanKind};
use formats::{Alpha, AlphaKind, Pixel, Span};

use crate::{
    array_cursor::ArrayCursor,
    geometry::{URect, Vec2, W16},
    range_ext::RangeExt,
};

mod array_cursor;
pub mod geometry;
mod range_ext;

mod sealed {
    pub trait Sealed {}
}

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
    row_starts: &'a [RowStart],
    _k: PhantomData<fn() -> K>,
}

impl<'a, Px, K: AlphaKind> Image<'a, Px, K> {
    pub const fn new(
        width: u16,
        pixels: &'a [Px],
        alphas: &'a [u8],
        spans: &'a [Span],
        row_starts: &'a [RowStart],
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

    pub fn size(&self) -> Vec2<u16> {
        Vec2::new(self.width(), self.height())
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

fn copy_iter<'a, T: Copy + 'a>(iter: impl Iterator<Item = (&'a mut T, &'a T)>) {
    for (d, &s) in iter {
        *d = s
    }
}

fn blend_iter<'a, Px: Copy + 'a, K: DrawAlpha<Px>>(
    iter: impl Iterator<Item = (&'a mut Px, (&'a Px, &'a u8))>,
) {
    for (d, (&s, &alpha)) in iter {
        *d = K::blend(*d, s, alpha);
    }
}

fn write_pixels<Px: Copy>(buf: &mut [Px], start: usize, step: u16, flipped: bool, src: &[Px]) {
    match step {
        1 => {
            let dst = &mut buf[start..start + src.len()];
            if flipped {
                copy_iter(dst.iter_mut().rev().zip(src));
            } else {
                dst.copy_from_slice(src);
            }
        }
        s => {
            let iter = buf[start..].iter_mut().step_by(s as usize).take(src.len());
            if flipped {
                copy_iter(iter.rev().zip(src));
            } else {
                copy_iter(iter.zip(src));
            }
        }
    }
}

fn blend_pixels<Px: Copy, K: DrawAlpha<Px>>(
    buf: &mut [Px],
    start: usize,
    step: u16,
    flipped: bool,
    src: &[Px],
    alphas: &[u8],
) {
    let src = src.iter().zip(alphas);
    match step {
        1 => {
            let dst = &mut buf[start..start + src.len()];
            if flipped {
                blend_iter::<Px, K>(dst.iter_mut().rev().zip(src));
            } else {
                blend_iter::<Px, K>(dst.iter_mut().zip(src));
            }
        }
        s => {
            let iter = buf[start..].iter_mut().step_by(s as usize).take(src.len());
            if flipped {
                blend_iter::<Px, K>(iter.rev().zip(src));
            } else {
                blend_iter::<Px, K>(iter.zip(src));
            }
        }
    }
}

pub trait DrawAlpha<Px>: AlphaKind + sealed::Sealed {
    fn blend(dst: Px, src: Px, alpha: u8) -> Px;
}

impl sealed::Sealed for Opaque {}
impl<Px> DrawAlpha<Px> for Opaque {
    fn blend(_: Px, src: Px, _: u8) -> Px {
        src
    }
}

impl sealed::Sealed for Masked {}
impl<Px> DrawAlpha<Px> for Masked {
    fn blend(_: Px, src: Px, _: u8) -> Px {
        src
    }
}

impl sealed::Sealed for Blended {}
impl<Px: Blend> DrawAlpha<Px> for Blended {
    fn blend(dst: Px, src: Px, alpha: u8) -> Px {
        dst.blend(src, alpha)
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

    pub fn draw_image<K: DrawAlpha<Px>>(
        &mut self,
        image: &Image<'_, Px, K>,
        pos: Vec2<i16>,
        transform: Transform,
    ) {
        let Vec2 {
            x: out_width,
            y: out_height,
        } = transform.swap_if_transposed(image.size());
        let step = if transform.is_transposed() {
            self.width()
        } else {
            1
        };
        let flipped = transform.src_reversed().x;
        let Some(out_x) = (0..self.width() as i16).intersection(pos.x..pos.x + out_width as i16)
        else {
            return;
        };
        let Some(out_y) = self
            .band
            .clone()
            .cast()
            .intersection(pos.y..pos.y + out_height as i16)
        else {
            return;
        };
        let (mut in_x, mut in_y) = (out_x.clone().sub(pos.x), out_y.clone().sub(pos.y));
        if transform.is_flipped_x() {
            in_x = in_x.sub_from(out_width as i16);
        }
        if transform.is_flipped_y() {
            in_y = in_y.sub_from(out_height as i16);
        }
        (in_x, in_y) = transform.swap_if_transposed((in_x, in_y));
        match K::KIND {
            Alpha::Opaque => {
                let mut px = ArrayCursor::new(in_y.start as u32 * image.width() as u32);
                for (mut y, span) in image.spans[in_y.start as usize..in_y.end as usize]
                    .iter()
                    .enumerate()
                {
                    if transform.src_reversed().y {
                        y = in_y.len() - 1 - y;
                    }
                    let (mut x, mut y) = transform.swap_if_transposed((0, y as u16));
                    x += out_x.start as u16;
                    y += out_y.start as u16;
                    let row_range = in_x.clone().cast();
                    let src = &image.pixels[px.step(span.len)][row_range];
                    let start = self.buffer_index(Vec2::new(x, y));
                    write_pixels(self.buffer.as_mut(), start, step, flipped, src);
                }
            }
            Alpha::Masked | Alpha::Blended => {
                let in_x = in_x.cast::<u16>();
                let map = |sx: u16, sy: u16| {
                    let (mut x, mut y) = transform.swap_if_transposed((sx, sy));
                    if transform.is_flipped_x() {
                        x = out_width - 1 - x;
                    }
                    if transform.is_flipped_y() {
                        y = out_height - 1 - y;
                    }
                    Vec2::new((pos.x + x as i16) as u16, (pos.y + y as i16) as u16)
                };
                for src_y in in_y.cast::<u16>() {
                    let row_start = &image.row_starts[src_y as usize];
                    let spans_end = image
                        .row_starts
                        .get(src_y as usize + 1)
                        .map(|s| s.span_idx as usize)
                        .unwrap_or(image.spans.len());
                    let mut px = ArrayCursor::new(row_start.px_idx);
                    let mut alpha = ArrayCursor::new(row_start.alpha_idx);
                    let mut src_x = 0;
                    for span in &image.spans[row_start.span_idx as usize..spans_end] {
                        if src_x >= in_x.end {
                            break;
                        }
                        let span_range = src_x..src_x + span.len;
                        src_x += span.len;
                        if span.kind == SpanKind::Transparent {
                            continue;
                        }
                        let px_range = px.step(span.len);
                        let alpha_range = if span.kind == SpanKind::Blended {
                            if K::KIND != Alpha::Blended {
                                panic!("Unexpected SpanKind::Blended span in Alpha::Masked image");
                            }
                            alpha.step(span.len)
                        } else {
                            0..0
                        };
                        let Some(range) = span_range.clone().intersection(in_x.clone()) else {
                            continue;
                        };
                        let span_offset = range.clone().sub(span_range.start).cast::<usize>();
                        let src = &image.pixels[px_range][span_offset.clone()];
                        let first = if flipped { range.end - 1 } else { range.start };
                        let start = self.buffer_index(map(first, src_y));
                        if span.kind == SpanKind::Blended {
                            let alphas = &image.alphas[alpha_range][span_offset];
                            blend_pixels::<Px, K>(
                                self.buffer.as_mut(),
                                start,
                                step,
                                flipped,
                                src,
                                alphas,
                            );
                        } else {
                            write_pixels(self.buffer.as_mut(), start, step, flipped, src);
                        }
                    }
                }
            }
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

pub trait Transpose {
    fn transposed(self) -> Self;
}

impl<T> Transpose for (T, T) {
    #[inline]
    fn transposed(self) -> Self {
        (self.1, self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Transform {
    // Bits (most to least significant): flip x, flip y, transpose (swap x and y)
    /// No transformation
    Identity = 0b000,
    /// Mirror along a 45° diagonal bisecting the top left corner
    Transpose = 0b001,
    /// Flip along the Y axis
    FlipY = 0b010,
    /// Rotate 90° counterclockwise
    RotCcw = 0b011,
    /// Flip along the X axis
    FlipX = 0b100,
    /// Rotate 90° clockwise
    RotCw = 0b101,
    /// Rotate 180°
    Rot180 = 0b110,
    /// Mirror along a 45° diagonal bisecting the top right corner
    Transverse = 0b111,
}

impl Transform {
    const fn from_u8(value: u8) -> Self {
        match value {
            0b000 => Self::Identity,
            0b001 => Self::Transpose,
            0b010 => Self::FlipY,
            0b011 => Self::RotCcw,
            0b100 => Self::FlipX,
            0b101 => Self::RotCw,
            0b110 => Self::Rot180,
            0b111 => Self::Transverse,
            _ => unreachable!(),
        }
    }

    #[inline]
    pub const fn from_bits(flip_x: bool, flip_y: bool, transpose: bool) -> Self {
        Self::from_u8((flip_x as u8) << 2 | (flip_y as u8) << 1 | transpose as u8)
    }

    #[inline]
    pub const fn is_flipped_x(self) -> bool {
        self as u8 & 0b100 != 0
    }

    #[inline]
    pub const fn is_flipped_y(self) -> bool {
        self as u8 & 0b010 != 0
    }

    #[inline]
    pub const fn is_transposed(self) -> bool {
        self as u8 & 0b001 != 0
    }

    pub fn swap_if_transposed<T: Transpose>(self, value: T) -> T {
        if self.is_transposed() {
            value.transposed()
        } else {
            value
        }
    }

    pub fn src_reversed(self) -> Vec2<bool> {
        self.swap_if_transposed(Vec2::new(self.is_flipped_x(), self.is_flipped_y()))
    }
}

macro_rules! impl_transform_bit_ops {
    ($($op:ident),+ $(,)?) => {
        $(pastey::paste! {
            impl core::ops::$op for Transform {
                type Output = Self;

                #[inline]
                fn [<$op:lower>](self, other: Self) -> Self {
                    Self::from_u8((self as u8).[<$op:lower>](other as u8))
                }
            }

            impl core::ops::[<$op Assign>] for Transform {
                #[inline]
                fn [<$op:lower _assign>](&mut self, other: Self) {
                    *self = self.[<$op:lower>](other);
                }
            }
        })*
    };
}

impl_transform_bit_ops!(BitAnd, BitOr, BitXor);

impl Not for Transform {
    type Output = Self;

    fn not(self) -> Self {
        Self::from_u8(!(self as u8) & 0b111)
    }
}
