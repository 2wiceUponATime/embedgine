#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

use core::{fmt::Debug, marker::PhantomData};

use crate::geometry::Vec2;

#[cfg(feature = "embedded-graphics")]
pub mod embedded_graphics;
pub mod geometry;

mod sealed {
    pub trait Sealed {}

    pub trait PodSlice {}
    impl<T: bytemuck::Pod> PodSlice for [T] {}
}

pub mod __private {
    #[cfg(feature = "alloc")]
    pub use alloc::vec::Vec;

    pub mod forward_pixel_array_autocomplete {
        pub use crate::{PixelArray, PixelBuffer};
    }
}

macro_rules! impl_debug_tuple {
    ($ty:ident => $name:ident ( $($field:ident),* $(,)? )) => {
        impl Debug for $ty {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.debug_tuple(stringify!($name))
                    $(.field(&self.$field))*
                    .finish()
            }
        }
    };
}

macro_rules! alpha {
    ($($name:ident),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
        #[repr(u8)]
        pub enum Alpha {
            #[default]
            $($name),+
        }

        pub trait AlphaKind: sealed::Sealed + Debug {
            const KIND: Alpha;
        }

        $(
            #[derive(Debug)]
            pub struct $name;

            impl sealed::Sealed for $name {}
            impl AlphaKind for $name {
                const KIND: Alpha = Alpha::$name;
            }
        )+
    };
}

alpha!(Opaque, Masked, Blended);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum SpanKind {
    Opaque,
    Transparent,
    Blended,
}

impl SpanKind {
    pub fn from_alpha(alpha: u8) -> Self {
        match alpha {
            255 => Self::Opaque,
            0 => Self::Transparent,
            _ => Self::Blended,
        }
    }
}

impl From<SpanKind> for Alpha {
    fn from(value: SpanKind) -> Self {
        match value {
            SpanKind::Opaque => Self::Opaque,
            SpanKind::Transparent => Self::Masked,
            SpanKind::Blended => Self::Blended,
        }
    }
}

#[derive(Debug)]
pub struct Span {
    pub len: u16,
    pub kind: SpanKind,
}

#[derive(Debug)]
pub struct RowStart {
    pub px_idx: u32,
    pub alpha_idx: u32,
    pub span_idx: u16,
}

#[derive(Clone, Copy, Default, PartialEq)]
#[repr(C)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_values([r, g, b, a]: [u8; 4]) -> Self {
        Self::new(r, g, b, a)
    }
}

impl_debug_tuple!(Rgba8 => rgba8(r, g, b, a));

pub trait PixelArray<T> {
    fn max_len(&self) -> usize;

    fn read(&self, idx: usize) -> T;

    fn pixels_from(&self, start: usize) -> impl Iterator<Item = T>;
}

pub trait PackedPixelArray<T>: PixelArray<T> + sealed::PodSlice {
    #[cfg(feature = "alloc")]
    fn pack(pixels: alloc::vec::Vec<T>) -> alloc::vec::Vec<u8>;
}

const fn assert_pixel_byte_align<W>() {
    assert!(
        core::mem::align_of::<W>() == 1,
        "pixel storage must have alignment 1",
    );
}

#[doc(hidden)]
pub const unsafe fn cast_bytes<W: bytemuck::Pod>(bytes: &[u8]) -> &[W] {
    let size = core::mem::size_of::<W>();
    assert_pixel_byte_align::<W>();
    assert!(
        size != 0 && bytes.len().is_multiple_of(size),
        "length is not a whole number of words",
    );
    unsafe { core::slice::from_raw_parts(bytes.as_ptr().cast(), bytes.len() / size) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    X,
    XRev,
    Y,
    YRev,
}

impl Dir {
    fn as_vec(self) -> Vec2<i8> {
        match self {
            Dir::X => Vec2::new(1, 0),
            Dir::XRev => Vec2::new(-1, 0),
            Dir::Y => Vec2::new(0, 1),
            Dir::YRev => Vec2::new(0, -1),
        }
    }
}

pub trait PixelBuffer<T> {
    fn capacity(&self) -> Vec2<u16>;

    fn contains_pos(&self, pos: Vec2<u16>) -> bool;

    fn read_pos(&self, pos: Vec2<u16>) -> T;

    fn write(&mut self, pos: Vec2<u16>, value: T);

    fn write_from(&mut self, start: Vec2<u16>, dir: Dir, src: impl IntoIterator<Item = T>) {
        let step = dir.as_vec().cast::<i32>();
        let mut pos = start.cast::<i32>();
        for v in src {
            let Ok(upos) = pos.try_cast::<u16>() else {
                break;
            };
            if !self.contains_pos(upos) {
                break;
            }
            self.write(upos, v);
            pos += step;
        }
    }

    fn update_from(
        &mut self,
        start: Vec2<u16>,
        dir: Dir,
        src: impl IntoIterator<Item = T>,
        f: impl Fn(usize, T, T) -> T,
    ) {
        let step = dir.as_vec().cast::<i32>();
        let mut pos = start.cast::<i32>();
        for (i, v) in src.into_iter().enumerate() {
            let Ok(upos) = pos.try_cast::<u16>() else {
                break;
            };
            if !self.contains_pos(upos) {
                break;
            }
            self.write(upos, f(i, self.read_pos(upos), v));
            pos += step;
        }
    }
}

impl<T: Copy> PixelArray<T> for [T] {
    #[inline]
    fn max_len(&self) -> usize {
        self.len()
    }

    #[inline]
    fn read(&self, idx: usize) -> T {
        self[idx]
    }

    #[inline]
    fn pixels_from(&self, start: usize) -> impl Iterator<Item = T> {
        self[start..].iter().copied()
    }
}

impl<T: bytemuck::Pod> PackedPixelArray<T> for [T] {
    #[cfg(feature = "alloc")]
    #[inline]
    fn pack(pixels: alloc::vec::Vec<T>) -> alloc::vec::Vec<u8> {
        assert_pixel_byte_align::<T>();
        bytemuck::cast_vec(pixels)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ArrayBuffer<S> {
    pub storage: S,
    pub width: u16,
}

impl<S> ArrayBuffer<S> {
    pub fn new(storage: S, width: u16) -> Self {
        Self { storage, width }
    }

    pub fn height<U>(&self) -> u16
    where
        S: AsRef<[U]>,
    {
        (self.storage.as_ref().len() / self.width as usize) as u16
    }

    pub fn size<U>(&self) -> Vec2<u16>
    where
        S: AsRef<[U]>,
    {
        Vec2::new(self.width, self.height())
    }

    pub fn as_ref(&self) -> ArrayBuffer<&S> {
        ArrayBuffer::new(&self.storage, self.width)
    }

    pub fn as_mut(&mut self) -> ArrayBuffer<&mut S> {
        ArrayBuffer::new(&mut self.storage, self.width)
    }

    fn storage_index(&self, pos: Vec2<u16>) -> usize {
        pos.y as usize * self.width as usize + pos.x as usize
    }
}

#[cfg(feature = "alloc")]
impl<T: Default + Clone> ArrayBuffer<alloc::vec::Vec<T>> {
    pub fn with_size(size: Vec2<u16>) -> Self {
        Self::new(
            alloc::vec![T::default(); size.x as usize * size.y as usize],
            size.x,
        )
    }
}

forward_pixel_array! {
    impl[T, U: PixelArray<T>] PixelArray<T> for ArrayBuffer<U> => |s| &s.storage;
}

#[cfg(feature = "alloc")]
impl<T: Copy> PixelBuffer<T> for ArrayBuffer<alloc::vec::Vec<T>> {
    #[inline]
    fn capacity(&self) -> Vec2<u16> {
        self.size()
    }

    #[inline]
    fn contains_pos(&self, pos: Vec2<u16>) -> bool {
        pos.x < self.width && pos.y < self.height()
    }

    #[inline]
    fn read_pos(&self, pos: Vec2<u16>) -> T {
        self.storage[self.storage_index(pos)]
    }

    #[inline]
    fn write(&mut self, pos: Vec2<u16>, value: T) {
        let i = self.storage_index(pos);
        self.storage[i] = value;
    }
}

impl<T: Copy> PixelBuffer<T> for ArrayBuffer<&mut [T]> {
    #[inline]
    fn capacity(&self) -> Vec2<u16> {
        self.size()
    }

    #[inline]
    fn contains_pos(&self, pos: Vec2<u16>) -> bool {
        pos.x < self.width && pos.y < self.height()
    }

    #[inline]
    fn read_pos(&self, pos: Vec2<u16>) -> T {
        self.storage[self.storage_index(pos)]
    }

    #[inline]
    fn write(&mut self, pos: Vec2<u16>, value: T) {
        self.storage[self.storage_index(pos)] = value;
    }

    // TODO: faster write implementations
}

#[macro_export]
macro_rules! forward_pixel_array {
    (
        $(impl $([$($gen:tt)*])? $trait:ident<$t:ty> for $outer:ty
            => |$s:ident| $inner:expr;)+
    ) => {
        $(
            const _: () = {
                #[allow(unused)]
                use $crate::__private::forward_pixel_array_autocomplete::$trait as _;
            };
            $crate::forward_pixel_array!(@item impl[$($($gen)*)?] $trait<$t> for $outer => |$s| $inner);
        )+
    };

    (impl< $($rest:tt)*) => {
        ::core::compile_error!(
            "write generics in brackets: `impl[T] PixelArray<T> for ...`"
        )
    };

    (@item impl[$($gen:tt)*] PixelArray<$t:ty> for $outer:ty => |$s:ident| $inner:expr) => {
        impl<$($gen)*> $crate::PixelArray<$t> for $outer {
            #[inline]
            fn max_len(&self) -> usize {
                let $s = self;
                $crate::PixelArray::<$t>::max_len($inner)
            }

            #[inline]
            fn read(&self, idx: usize) -> $t {
                let $s = self;
                $crate::PixelArray::<$t>::read($inner, idx)
            }

            #[inline]
            fn pixels_from(&self, start: usize) -> impl ::core::iter::Iterator<Item = $t> {
                let $s = self;
                $crate::PixelArray::<$t>::pixels_from($inner, start)
            }
        }
    };

    (@item impl[$($gen:tt)*] PixelBuffer<$t:ty> for $outer:ty => |$s:ident| $inner:expr) => {
        impl<$($gen)*> $crate::PixelBuffer<$t> for $outer {
            fn capacity(&self) -> $crate::geometry::Vec2<u16> {
                let $s = self;
                $crate::PixelBuffer::<$t>::capacity(&$inner)
            }

            fn contains_pos(&self, pos: $crate::geometry::Vec2<u16>) -> bool {
                let $s = self;
                $crate::PixelBuffer::<$t>::contains_pos(&$inner, pos)
            }

            fn read_pos(&self, pos: $crate::geometry::Vec2<u16>) -> $t {
                let $s = self;
                $crate::PixelBuffer::<$t>::read_pos(&$inner, pos)
            }

            fn write(&mut self, pos: $crate::geometry::Vec2<u16>, value: $t) {
                let $s = self;
                $crate::PixelBuffer::<$t>::write(&mut $inner, pos, value);
            }

            fn write_from(&mut self, start: $crate::geometry::Vec2<u16>, dir: $crate::Dir, src: impl ::core::iter::IntoIterator<Item = $t>) {
                let $s = self;
                $crate::PixelBuffer::<$t>::write_from(&mut $inner, start, dir, src);
            }

            fn update_from(
                &mut self,
                start: $crate::geometry::Vec2<u16>,
                dir: $crate::Dir,
                src: impl ::core::iter::IntoIterator<Item = $t>,
                f: impl ::core::ops::Fn(usize, $t, $t) -> $t,
            ) {
                let $s = self;
                $crate::PixelBuffer::<$t>::update_from(&mut $inner, start, dir, src, f);
            }
        }
    };

    (@item impl[$($gen:tt)*] $trait:ident $($rest:tt)*) => {
        ::core::compile_error!(concat!(
            "forward_pixel_array! can only forward `PixelArray` and `PixelBuffer`, not `",
            stringify!($trait),
            "`",
        ))
    }
}

forward_pixel_array! {
    impl[T: Copy, const N: usize] PixelArray<T> for [T; N] => |s| s.as_slice();

    impl[T, U: PixelArray<T>] PixelArray<T> for &U => |s| *s;
    impl[T, U: PixelArray<T>] PixelArray<T> for &mut U => |s| *s;
    impl[T, U: PixelBuffer<T>] PixelBuffer<T> for &mut U => |s| **s;
}

#[cfg(feature = "alloc")]
forward_pixel_array! {
    impl[T: bytemuck::Pod] PixelArray<T> for alloc::vec::Vec<T> => |s| s.as_slice();
}

pub trait Pixel: Debug + Copy + Default + bytemuck::Pod + From<Rgb888> {
    type Array: ?Sized + PackedPixelArray<Self>;

    const FORMAT: &'static str;
}

pub trait Blend: Pixel {
    fn blend(self, src: Self, alpha: u8) -> Self;
}

#[derive(Clone, Copy, Default, PartialEq, Eq, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Rgb888 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb888 {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

impl_debug_tuple!(Rgb888 => rgb888(r, g, b));

impl From<Rgba8> for Rgb888 {
    fn from(Rgba8 { r, g, b, .. }: Rgba8) -> Self {
        Self { r, g, b }
    }
}

impl Pixel for Rgb888 {
    type Array = [Self];

    const FORMAT: &'static str = "rgb888";
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(transparent)]
pub struct BinaryColor(u8);

impl BinaryColor {
    pub const OFF: Self = Self(0);
    pub const ON: Self = Self(1);

    pub const fn is_on(self) -> bool {
        self.0 != 0
    }

    pub fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        let y = (r as u16 * 54 + g as u16 * 183 + b as u16 * 19) >> 8;
        (y >= 128).into()
    }

    #[cfg(feature = "alloc")]
    pub fn packed_vec(len: usize) -> alloc::vec::Vec<u8> {
        alloc::vec![0; len.div_ceil(8)]
    }
}

impl From<bool> for BinaryColor {
    fn from(value: bool) -> Self {
        if value { Self::ON } else { Self::OFF }
    }
}

impl From<BinaryColor> for bool {
    fn from(value: BinaryColor) -> Self {
        value.is_on()
    }
}

impl From<Rgb888> for BinaryColor {
    fn from(Rgb888 { r, g, b }: Rgb888) -> Self {
        Self::from_rgb8(r, g, b)
    }
}

impl Pixel for BinaryColor {
    type Array = [u8];

    const FORMAT: &'static str = "binary";
}

impl PixelArray<BinaryColor> for [u8] {
    #[inline]
    fn max_len(&self) -> usize {
        self.len() * 8
    }

    #[inline]
    fn read(&self, idx: usize) -> BinaryColor {
        (self[idx / 8] & 1 << (idx % 8) != 0).into()
    }

    #[inline]
    fn pixels_from(&self, start: usize) -> impl Iterator<Item = BinaryColor> {
        let mut i = start;
        core::iter::from_fn(move || {
            if i / 8 >= self.len() {
                None
            } else {
                let value = self[i / 8] & 1 << (i % 8) != 0;
                i += 1;
                Some(value.into())
            }
        })
    }
}

impl PackedPixelArray<BinaryColor> for [u8] {
    #[cfg(feature = "alloc")]
    fn pack(pixels: alloc::vec::Vec<BinaryColor>) -> alloc::vec::Vec<u8> {
        let mut buf = BinaryColor::packed_vec(pixels.len());
        for (i, &p) in pixels.iter().enumerate() {
            if p.is_on() {
                buf[i / 8] |= 1 << (i % 8);
            }
        }
        buf
    }
}

pub trait BitOrder: sealed::Sealed {
    fn map_bit(i: u8) -> u8;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Msb0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lsb0;

impl sealed::Sealed for Lsb0 {}
impl BitOrder for Lsb0 {
    #[inline(always)]
    fn map_bit(i: u8) -> u8 {
        i
    }
}

impl sealed::Sealed for Msb0 {}
impl BitOrder for Msb0 {
    #[inline(always)]
    fn map_bit(i: u8) -> u8 {
        7 - i
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BinaryBuffer<S, O: BitOrder> {
    pub storage: S,
    pub width: u16,
    pub pad_rows: bool,
    _order: PhantomData<O>,
}

impl<S, O: BitOrder> BinaryBuffer<S, O> {
    pub fn new(storage: S, width: u16, pad_rows: bool) -> Self {
        Self {
            storage,
            width,
            pad_rows,
            _order: PhantomData,
        }
    }

    fn row_bits(&self) -> u16 {
        if self.pad_rows {
            self.width.next_multiple_of(8)
        } else {
            self.width
        }
    }

    fn bit_index(&self, Vec2 { x, y }: Vec2<u16>) -> usize {
        self.row_bits() as usize * y as usize + x as usize
    }

    fn bit_mask(bit: usize) -> u8 {
        1 << O::map_bit((bit % 8) as u8)
    }
}

impl<S: AsRef<[u8]> + AsMut<[u8]>, O: BitOrder> PixelBuffer<BinaryColor> for BinaryBuffer<S, O> {
    fn capacity(&self) -> Vec2<u16> {
        let height = match self.row_bits() {
            0 => 0,
            row_bits => self.storage.as_ref().len() * 8 / row_bits as usize,
        };
        Vec2::new(self.width, height.min(u16::MAX as usize) as u16)
    }

    #[inline]
    fn contains_pos(&self, pos: Vec2<u16>) -> bool {
        pos.x < self.width && pos.y < self.capacity().y
    }

    #[inline]
    fn read_pos(&self, pos: Vec2<u16>) -> BinaryColor {
        let bit = self.bit_index(pos);
        (self.storage.as_ref()[bit / 8] & Self::bit_mask(bit) != 0).into()
    }

    #[inline]
    fn write(&mut self, pos: Vec2<u16>, value: BinaryColor) {
        let bit = self.bit_index(pos);
        let mask = Self::bit_mask(bit);
        let byte = &mut self.storage.as_mut()[bit / 8];
        if value.is_on() {
            *byte |= mask;
        } else {
            *byte &= !mask;
        }
    }
}

#[cfg(feature = "alloc")]
forward_pixel_array! {
    impl PixelArray<BinaryColor> for alloc::vec::Vec<u8> => |s| s.as_slice();
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: BinaryColor = BinaryColor::ON;
    const OFF: BinaryColor = BinaryColor::OFF;

    fn read_bit(buf: &[u8], idx: usize) -> BinaryColor {
        PixelArray::<BinaryColor>::read(buf, idx)
    }

    fn pos(x: u16, y: u16) -> Vec2<u16> {
        Vec2::new(x, y)
    }

    #[test]
    fn span_kind_from_alpha() {
        assert_eq!(SpanKind::from_alpha(0), SpanKind::Transparent);
        assert_eq!(SpanKind::from_alpha(1), SpanKind::Blended);
        assert_eq!(SpanKind::from_alpha(254), SpanKind::Blended);
        assert_eq!(SpanKind::from_alpha(255), SpanKind::Opaque);
    }

    #[test]
    fn span_kind_maps_to_required_image_alpha() {
        assert_eq!(Alpha::from(SpanKind::Opaque), Alpha::Opaque);
        assert_eq!(Alpha::from(SpanKind::Transparent), Alpha::Masked);
        assert_eq!(Alpha::from(SpanKind::Blended), Alpha::Blended);
        // The build encoder relies on this order to pick an image's alpha kind.
        assert!(Alpha::Opaque < Alpha::Masked && Alpha::Masked < Alpha::Blended);
    }

    #[test]
    fn array_buffer_size_comes_from_storage_and_width() {
        let mut storage = [0u8; 6];
        let buf = ArrayBuffer::new(&mut storage[..], 3);
        assert_eq!(buf.capacity(), pos(3, 2));
        assert!(buf.contains_pos(pos(2, 1)));
        assert!(!buf.contains_pos(pos(3, 0)));
        assert!(!buf.contains_pos(pos(0, 2)));
    }

    #[test]
    fn array_write_from_starts_at_start_and_stops_at_each_edge() {
        // From the centre of a 3x3 grid, each direction has room for two pixels.
        for (dir, expected) in [
            (Dir::X, [0, 0, 0, 0, 1, 2, 0, 0, 0]),
            (Dir::XRev, [0, 0, 0, 2, 1, 0, 0, 0, 0]),
            (Dir::Y, [0, 0, 0, 0, 1, 0, 0, 2, 0]),
            (Dir::YRev, [0, 2, 0, 0, 1, 0, 0, 0, 0]),
        ] {
            let mut storage = [0u8; 9];
            ArrayBuffer::new(&mut storage[..], 3).write_from(pos(1, 1), dir, 1..);
            assert_eq!(storage, expected, "{dir:?}");
        }
    }

    #[test]
    fn array_write_from_does_not_wrap_onto_the_next_row() {
        let mut storage = [0u8; 4];
        ArrayBuffer::new(&mut storage[..], 2).write_from(pos(1, 0), Dir::X, [7, 7]);
        assert_eq!(storage, [0, 7, 0, 0]);
    }

    #[test]
    fn array_write_from_outside_the_buffer_writes_nothing() {
        let mut storage = [0u8; 4];
        let mut buf = ArrayBuffer::new(&mut storage[..], 2);
        buf.write_from(pos(2, 0), Dir::XRev, [7]);
        buf.write_from(pos(0, 2), Dir::YRev, [7]);
        assert_eq!(storage, [0; 4]);
    }

    #[test]
    fn array_update_from_passes_source_index_dst_and_src() {
        let mut storage = [10u8, 20, 30, 40];
        ArrayBuffer::new(&mut storage[..], 4).update_from(
            pos(1, 0),
            Dir::X,
            [1, 2],
            |i, dst, src| dst + src + i as u8 * 100,
        );
        assert_eq!(storage, [10, 21, 132, 40]);
    }

    #[test]
    fn array_update_from_stops_at_the_edge() {
        let mut storage = [10u8, 20, 30, 40];
        ArrayBuffer::new(&mut storage[..], 2).update_from(
            pos(1, 1),
            Dir::YRev,
            [1, 2, 3],
            |_, dst, src| dst + src,
        );
        assert_eq!(storage, [10, 22, 30, 41]);
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn vec_array_buffer_behaves_like_slice_buffer() {
        let mut buf = ArrayBuffer::<alloc::vec::Vec<u8>>::with_size(pos(3, 1));
        buf.write_from(pos(1, 0), Dir::X, core::iter::repeat(9));
        assert_eq!(buf.storage, [0, 9, 9]);
        buf.update_from(pos(0, 0), Dir::X, [1], |_, dst, src| dst + src);
        assert_eq!(buf.read_pos(pos(0, 0)), 1);
        assert_eq!(buf.capacity(), pos(3, 1));
    }

    #[test]
    fn binary_luminance_threshold() {
        assert!(!BinaryColor::from_rgb8(0, 0, 0).is_on());
        assert!(BinaryColor::from_rgb8(255, 255, 255).is_on());
        // Green carries most of the luminance; red and blue alone stay below half.
        assert!(BinaryColor::from_rgb8(0, 255, 0).is_on());
        assert!(!BinaryColor::from_rgb8(255, 0, 0).is_on());
        assert!(!BinaryColor::from_rgb8(0, 0, 255).is_on());
    }

    #[test]
    fn binary_bool_round_trip() {
        assert_eq!(BinaryColor::from(true), ON);
        assert_eq!(BinaryColor::from(false), OFF);
        assert!(bool::from(ON));
        assert!(!bool::from(OFF));
    }

    #[test]
    fn packed_max_len_counts_bits() {
        assert_eq!(PixelArray::<BinaryColor>::max_len(&[0u8; 3][..]), 24);
    }

    #[test]
    fn packed_pixels_from_starts_mid_byte_and_ends_at_last_bit() {
        let buf = [0b1000_0000u8, 0b0000_0001];
        let mut bits = PixelArray::<BinaryColor>::pixels_from(&buf[..], 6);
        assert_eq!(bits.next(), Some(OFF));
        assert_eq!(bits.next(), Some(ON));
        assert_eq!(bits.next(), Some(ON));
        assert_eq!(bits.by_ref().count(), 7);
        assert_eq!(bits.next(), None);
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn packed_vec_rounds_up_to_whole_bytes() {
        for (len, bytes) in [(0, 0), (1, 1), (8, 1), (9, 2), (16, 2), (17, 3)] {
            assert_eq!(BinaryColor::packed_vec(len).len(), bytes, "{len} pixels");
        }
    }

    /// Runs a generic `BinaryBuffer` check over every bit order and row padding.
    macro_rules! for_each_layout {
        ($check:ident) => {
            $check::<Lsb0>(false);
            $check::<Lsb0>(true);
            $check::<Msb0>(false);
            $check::<Msb0>(true);
        };
    }

    fn binary<O: BitOrder>(
        storage: &mut [u8],
        width: u16,
        pad_rows: bool,
    ) -> BinaryBuffer<&mut [u8], O> {
        BinaryBuffer::new(storage, width, pad_rows)
    }

    #[test]
    fn binary_bit_layout_follows_order_and_padding() {
        // Width 3: (1, 0) is bit 1, and (0, 1) is bit 3 unpadded or bit 8 padded.
        fn layout<O: BitOrder>(pad_rows: bool) -> [u8; 2] {
            let mut storage = [0u8; 2];
            let mut buf = binary::<O>(&mut storage, 3, pad_rows);
            buf.write(pos(1, 0), ON);
            buf.write(pos(0, 1), ON);
            storage
        }
        assert_eq!(layout::<Lsb0>(false), [0b0000_1010, 0]);
        assert_eq!(layout::<Lsb0>(true), [0b0000_0010, 0b0000_0001]);
        assert_eq!(layout::<Msb0>(false), [0b0101_0000, 0]);
        assert_eq!(layout::<Msb0>(true), [0b0100_0000, 0b1000_0000]);
    }

    #[test]
    fn binary_capacity_is_the_visible_size() {
        fn check<O: BitOrder>(pad_rows: bool) {
            let mut storage = [0u8; 2];
            let buf = binary::<O>(&mut storage, 3, pad_rows);
            // 16 bits hold five 3-bit rows, or two 8-bit padded rows.
            let rows = if pad_rows { 2 } else { 5 };
            assert_eq!(buf.capacity(), pos(3, rows));
            assert!(buf.contains_pos(pos(2, rows - 1)));
            assert!(!buf.contains_pos(pos(3, 0)), "padding is not addressable");
            assert!(!buf.contains_pos(pos(0, rows)));
        }
        for_each_layout!(check);
    }

    #[test]
    fn binary_zero_width_has_no_pixels() {
        let mut storage = [0u8; 2];
        assert_eq!(binary::<Lsb0>(&mut storage, 0, false).capacity(), pos(0, 0));
    }

    #[test]
    fn binary_write_then_read_round_trips_every_pixel() {
        fn check<O: BitOrder>(pad_rows: bool) {
            let mut storage = [0u8; 4];
            let mut buf = binary::<O>(&mut storage, 5, pad_rows);
            let size = buf.capacity();
            let pattern = |x: u16, y: u16| BinaryColor::from((x + 2 * y).is_multiple_of(3));
            for y in 0..size.y {
                for x in 0..size.x {
                    buf.write(pos(x, y), pattern(x, y));
                }
            }
            buf.write(pos(1, 1), OFF);
            buf.write(pos(1, 1), ON);
            for y in 0..size.y {
                for x in 0..size.x {
                    let expected = if (x, y) == (1, 1) { ON } else { pattern(x, y) };
                    assert_eq!(
                        buf.read_pos(pos(x, y)),
                        expected,
                        "({x}, {y}) pad={pad_rows}"
                    );
                }
            }
        }
        for_each_layout!(check);
    }

    #[test]
    fn binary_write_from_stops_at_the_row_end() {
        fn check<O: BitOrder>(pad_rows: bool) {
            let mut storage = [0u8; 2];
            let mut buf = binary::<O>(&mut storage, 3, pad_rows);
            buf.write_from(pos(1, 0), Dir::X, core::iter::repeat(ON));
            assert_eq!(buf.read_pos(pos(0, 0)), OFF);
            assert_eq!(buf.read_pos(pos(1, 0)), ON);
            assert_eq!(buf.read_pos(pos(2, 0)), ON);
            assert_eq!(buf.read_pos(pos(0, 1)), OFF, "pad={pad_rows}");
            // Only the two written bits are set: nothing spilled into padding.
            assert_eq!(storage.iter().map(|b| b.count_ones()).sum::<u32>(), 2);
        }
        for_each_layout!(check);
    }

    #[test]
    fn binary_write_from_down_a_column_stops_at_the_bottom() {
        fn check<O: BitOrder>(pad_rows: bool) {
            let mut storage = [0u8; 2];
            let mut buf = binary::<O>(&mut storage, 3, pad_rows);
            let rows = buf.capacity().y;
            buf.write_from(pos(2, 0), Dir::Y, core::iter::repeat(ON));
            for y in 0..rows {
                assert_eq!(buf.read_pos(pos(2, y)), ON);
            }
            assert_eq!(
                storage.iter().map(|b| b.count_ones()).sum::<u32>(),
                rows as u32
            );
        }
        for_each_layout!(check);
    }

    #[test]
    fn binary_update_from_sees_current_pixels() {
        fn check<O: BitOrder>(pad_rows: bool) {
            let mut storage = [0u8; 2];
            let mut buf = binary::<O>(&mut storage, 3, pad_rows);
            buf.write(pos(1, 0), ON);
            // Invert, moving left from the row end and stopping at x = 0.
            buf.update_from(pos(2, 0), Dir::XRev, [ON; 4], |_, dst, _| {
                (!dst.is_on()).into()
            });
            assert_eq!(buf.read_pos(pos(0, 0)), ON);
            assert_eq!(buf.read_pos(pos(1, 0)), OFF);
            assert_eq!(buf.read_pos(pos(2, 0)), ON);
            assert_eq!(buf.read_pos(pos(0, 1)), OFF);
        }
        for_each_layout!(check);
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn binary_vec_storage_matches_packed_pixel_array() {
        let mut buf = BinaryBuffer::<_, Lsb0>::new(BinaryColor::packed_vec(9), 3, false);
        assert_eq!(buf.capacity(), pos(3, 5));
        buf.write(pos(2, 2), ON);
        assert_eq!(buf.read_pos(pos(2, 2)), ON);
        // Unpadded LSB-first matches the layout `PixelArray` reads.
        assert_eq!(read_bit(&buf.storage, 8), ON);
        assert_eq!(
            PixelArray::<BinaryColor>::pixels_from(&buf.storage, 8).next(),
            Some(ON)
        );
    }
}
