#![no_std]

#[cfg(any(feature = "alloc", test))]
extern crate alloc;

use core::{
    marker::PhantomData,
    ops::{BitAnd, BitOr, BitXor, Not},
    range::Range,
};

#[cfg(feature = "embedded-graphics")]
use embedded_graphics_core::draw_target::DrawTarget;
pub use embedgine_formats as formats;
use embedgine_formats::{
    Blend, Blended, Masked, Opaque, PixelArray, RowStart, SpanKind, WritablePixelArray,
};
use formats::{Alpha, AlphaKind, Pixel, Span};

use crate::{
    array_cursor::ArrayCursor,
    geometry::{URect, Vec2, W16},
    range_ext::RangeExt,
};

mod array_cursor;
pub mod geometry;
mod range_ext;
#[cfg(all(test, feature = "embedded-graphics"))]
mod tests;

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
pub struct Image<'a, Px: Pixel, K> {
    width: u16,
    pixels: &'a Px::Array,
    alphas: &'a [u8],
    spans: &'a [Span],
    row_starts: &'a [RowStart],
    _k: PhantomData<fn() -> K>,
}

impl<'a, Px: Pixel, K: AlphaKind> Image<'a, Px, K> {
    pub const fn new(
        width: u16,
        pixels: &'a Px::Array,
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

    pub fn pixels(&self) -> &Px::Array {
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

    fn present<B: WritablePixelArray<Self::Px>>(
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

    async fn present<B: WritablePixelArray<Px>>(
        &mut self,
        size: Vec2<u16>,
        region: URect<W16>,
        canvas: &Canvas<Self::Px, B>,
    ) -> Result<(), Self::Error> {
        assert_eq!(self.target.bounding_box().size, size.cast().into());
        let colors = (region.top()..region.bottom()).flat_map(|y| {
            (region.left()..region.right()).map(move |x| canvas.read(Vec2::new(x, y)).into())
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
    pub fn new(size: Vec2<u16>, band_height: u16, buffers: [B; 2]) -> Self {
        Self {
            size,
            band_height,
            buffers: buffers.map(|b| Canvas::new(size, (0..band_height).into(), b)),
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

    pub fn bands(height: u16, band_height: u16) -> impl Iterator<Item = Range<u16>> {
        (0..height)
            .step_by(band_height as usize)
            .map(move |y| (y..(y + band_height).min(height)).into())
    }

    pub fn resize(&mut self, size: Vec2<u16>, band_height: u16, buffers: [B; 2]) {
        self.size = size;
        self.band_height = band_height;
        self.buffers = buffers.map(|b| Canvas::new(size, (0..band_height).into(), b));
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
        B: WritablePixelArray<Px>,
    {
        let size = self.size;
        let mut canvas_idx = 0;
        let [mut drawing, mut presenting] = self.buffers.each_mut();
        let mut pending = None;
        for band in Self::bands(size.y, self.band_height) {
            drawing.band = band;
            draw.draw(drawing);
            if let Some(f) = { pending } {
                f.await?;
            }
            core::mem::swap(&mut drawing, &mut presenting);
            pending = Some(sink.present(
                size,
                URect::new(Vec2::new(0, band.start), Vec2::new(size.x, band.len())),
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

fn copy_iter<T: Copy>(
    iter: impl Iterator<Item = (usize, T)>,
    dst: &mut impl WritablePixelArray<T>,
) {
    for (i, s) in iter {
        dst.write(i, s);
    }
}

fn blend_iter<Px: Copy, K: DrawAlpha<Px>>(
    iter: impl Iterator<Item = (usize, (Px, u8))>,
    dst: &mut impl WritablePixelArray<Px>,
) {
    for (i, (s, alpha)) in iter {
        dst.write(i, K::blend(dst.read(i), s, alpha));
    }
}

fn write_pixels<Px: Copy>(
    buf: &mut impl WritablePixelArray<Px>,
    start: usize,
    step: u16,
    flipped: bool,
    len: usize,
    src: impl Iterator<Item = Px>,
) {
    match step {
        1 => {
            if flipped {
                copy_iter((start..start + len).rev().zip(src), buf);
            } else {
                buf.write_from(start, src);
            }
        }
        s => {
            let indices = (0..len).map(|i| start + i * (s as usize));
            if flipped {
                copy_iter(indices.rev().zip(src), buf);
            } else {
                copy_iter(indices.zip(src), buf);
            }
        }
    }
}

fn blend_pixels<Px: Copy, K: DrawAlpha<Px>>(
    buf: &mut impl WritablePixelArray<Px>,
    start: usize,
    step: u16,
    flipped: bool,
    len: usize,
    src: impl Iterator<Item = Px>,
    alphas: &[u8],
) {
    match step {
        1 => {
            let indices = start..start + len;
            if flipped {
                blend_iter::<Px, K>(indices.rev().zip(src.zip(alphas.iter().copied())), buf);
            } else {
                buf.update_from(start, src, |i, dst, src| K::blend(dst, src, alphas[i]));
            }
        }
        s => {
            let zipped = src.zip(alphas.iter().copied());
            let iter = (0..len).map(|i| start + i * (s as usize));
            if flipped {
                blend_iter::<Px, K>(iter.rev().zip(zipped), buf);
            } else {
                blend_iter::<Px, K>(iter.zip(zipped), buf);
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
        Self::new(size, (0..size.y).into(), buffer)
    }

    pub fn band(&self) -> Range<u16> {
        self.band
    }

    fn buffer_index(&self, Vec2 { x, y }: Vec2<u16>) -> usize {
        (y - self.band.start) as usize * self.size.x as usize + x as usize
    }
}

impl<Px: Pixel, B: WritablePixelArray<Px>> Canvas<Px, B> {
    pub fn fill(&mut self, color: Px) {
        self.buffer.write_from(0, core::iter::repeat(color));
    }

    pub fn draw_image<K: DrawAlpha<Px>>(
        &mut self,
        image: &Image<'_, Px, K>,
        pos: Vec2<i16>,
        transform: Transform,
    ) {
        let out_size = transform.swap_if_transposed(image.size());
        let step = if transform.is_transposed() {
            self.width()
        } else {
            1
        };
        let flipped = transform.src_reversed().x;
        let Some(out_x) = Range::from(0..self.width() as i16)
            .intersection((pos.x..pos.x + out_size.x as i16).into())
        else {
            return;
        };
        let Some(out_y) = self
            .band
            .cast()
            .intersection((pos.y..pos.y + out_size.y as i16).into())
        else {
            return;
        };
        let (mut in_x, mut in_y) = (out_x.sub(pos.x), out_y.sub(pos.y));
        if transform.is_flipped_x() {
            in_x = in_x.sub_from(out_size.x as i16);
        }
        if transform.is_flipped_y() {
            in_y = in_y.sub_from(out_size.y as i16);
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
                        y = in_y.len() as usize - 1 - y;
                    }
                    let dst = transform.swap_if_transposed(Vec2::new(0, y as u16))
                        + Vec2::new(out_x.start, out_y.start).cast::<u16>();
                    let row_range = in_x.cast::<usize>();
                    let src = image
                        .pixels
                        .pixels_from(px.step(span.len).start + row_range.start)
                        .take(row_range.len());
                    let start = self.buffer_index(dst);
                    write_pixels(&mut self.buffer, start, step, flipped, row_range.len(), src);
                }
            }
            Alpha::Masked | Alpha::Blended => {
                let in_x = in_x.cast::<u16>();
                let map = |sx: u16, sy: u16| {
                    let (mut x, mut y) = transform.swap_if_transposed((sx, sy));
                    if transform.is_flipped_x() {
                        x = out_size.x - 1 - x;
                    }
                    if transform.is_flipped_y() {
                        y = out_size.y - 1 - y;
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
                        let span_range = Range::from(src_x..src_x + span.len);
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
                            (0..0).into()
                        };
                        let Some(range) = span_range.intersection(in_x) else {
                            continue;
                        };
                        let span_offset = range.sub(span_range.start).cast::<usize>();
                        let src = image
                            .pixels
                            .pixels_from(px_range.start + span_offset.start)
                            .take(span_offset.len());
                        let first = if flipped { range.end - 1 } else { range.start };
                        let start = self.buffer_index(map(first, src_y));
                        if span.kind == SpanKind::Blended {
                            let alphas = &image.alphas[alpha_range][span_offset];
                            blend_pixels::<Px, K>(
                                &mut self.buffer,
                                start,
                                step,
                                flipped,
                                span_offset.len(),
                                src,
                                alphas,
                            );
                        } else {
                            write_pixels(
                                &mut self.buffer,
                                start,
                                step,
                                flipped,
                                span_offset.len(),
                                src,
                            );
                        }
                    }
                }
            }
        }
    }
}

impl<Px, B: WritablePixelArray<Px>> Canvas<Px, B> {
    pub fn read(&self, pos: Vec2<u16>) -> Px {
        self.buffer.read(self.buffer_index(pos))
    }

    pub fn write(&mut self, pos: Vec2<u16>, value: Px) {
        self.buffer.write(self.buffer_index(pos), value);
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

    #[inline]
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

    pub fn compose(self, other: Self) -> Self {
        let (flip_x, flip_y) = other.swap_if_transposed((self.is_flipped_x(), self.is_flipped_y()));

        Self::from_bits(
            flip_x ^ other.is_flipped_x(),
            flip_y ^ other.is_flipped_y(),
            self.is_transposed() ^ other.is_transposed(),
        )
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
