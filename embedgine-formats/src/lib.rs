#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

use core::fmt::Debug;

#[cfg(feature = "embedded-graphics")]
pub mod embedded_graphics;

mod sealed {
    pub trait Sealed {}

    pub trait PodSlice {}
    impl<T: bytemuck::Pod> PodSlice for [T] {}
}

pub mod __private {
    #[cfg(feature = "alloc")]
    pub use alloc::vec::Vec;

    pub mod forward_pixel_array_autocomplete {
        pub use crate::{PixelArray, WritablePixelArray};
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
    fn pack(pixels: &[T]) -> alloc::vec::Vec<u8>;
}

#[doc(hidden)]
pub const unsafe fn cast_bytes<W: bytemuck::Pod>(bytes: &[u8]) -> &[W] {
    let size = core::mem::size_of::<W>();
    assert!(
        core::mem::align_of::<W>() == 1,
        "pixel storage must have alignment 1",
    );
    assert!(
        size != 0 && bytes.len().is_multiple_of(size),
        "length is not a whole number of words",
    );
    unsafe { core::slice::from_raw_parts(bytes.as_ptr().cast(), bytes.len() / size) }
}

pub trait WritablePixelArray<T>: PixelArray<T> {
    fn write(&mut self, idx: usize, value: T);

    fn write_from(&mut self, start: usize, src: impl IntoIterator<Item = T>) {
        let remaining = self.max_len().saturating_sub(start);
        for (i, v) in src.into_iter().take(remaining).enumerate() {
            self.write(start + i, v);
        }
    }

    fn update_from(
        &mut self,
        start: usize,
        src: impl IntoIterator<Item = T>,
        f: impl Fn(usize, T, T) -> T,
    ) {
        let remaining = self.max_len().saturating_sub(start);
        for (i, v) in src.into_iter().take(remaining).enumerate() {
            let idx = start + i;
            self.write(idx, f(i, self.read(idx), v));
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
    fn pack(pixels: &[T]) -> alloc::vec::Vec<u8> {
        bytemuck::cast_slice(pixels).into()
    }
}

impl<T: bytemuck::Pod> WritablePixelArray<T> for [T] {
    #[inline]
    fn write(&mut self, idx: usize, value: T) {
        self[idx] = value;
    }

    #[inline]
    fn write_from(&mut self, start: usize, src: impl IntoIterator<Item = T>) {
        for (d, v) in self[start..].iter_mut().zip(src) {
            *d = v;
        }
    }

    #[inline]
    fn update_from(
        &mut self,
        start: usize,
        src: impl IntoIterator<Item = T>,
        f: impl Fn(usize, T, T) -> T,
    ) {
        for (i, (d, v)) in self[start..].iter_mut().zip(src).enumerate() {
            *d = f(i, *d, v);
        }
    }
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

    (@item impl[$($gen:tt)*] WritablePixelArray<$t:ty> for $outer:ty => |$s:ident| $inner:expr) => {
        impl<$($gen)*> $crate::WritablePixelArray<$t> for $outer {
            #[inline]
            fn write(&mut self, idx: usize, value: $t) {
                let $s = self;
                $crate::WritablePixelArray::<$t>::write($inner, idx, value);
            }

            #[inline]
            fn write_from(&mut self, start: usize, src: impl ::core::iter::IntoIterator<Item = $t>) {
                let $s = self;
                $crate::WritablePixelArray::<$t>::write_from($inner, start, src)
            }

            #[inline]
            fn update_from(
                &mut self,
                start: usize,
                src: impl ::core::iter::IntoIterator<Item = $t>,
                f: impl Fn(usize, $t, $t) -> $t,
            ) {
                let $s = self;
                $crate::WritablePixelArray::<$t>::update_from($inner, start, src, f);
            }
        }
    };

    (@item impl[$($gen:tt)*] $trait:ident $($rest:tt)*) => {
        ::core::compile_error!(concat!(
            "forward_pixel_array! can only forward `PixelArray` and `WritablePixelArray`, not `",
            stringify!($trait),
            "`",
        ))
    }
}

forward_pixel_array! {
    impl[T: bytemuck::Pod, const N: usize] PixelArray<T> for [T; N] => |s| s.as_slice();
    impl[T: bytemuck::Pod, const N: usize] WritablePixelArray<T> for [T; N] => |s| s.as_mut_slice();

    impl[T, U: PixelArray<T>] PixelArray<T> for &U => |s| *s;
    impl[T, U: PixelArray<T>] PixelArray<T> for &mut U => |s| *s;
    impl[T, U: WritablePixelArray<T>] WritablePixelArray<T> for &mut U => |s| *s;
}

#[cfg(feature = "alloc")]
forward_pixel_array! {
    impl[T: bytemuck::Pod] PixelArray<T> for alloc::vec::Vec<T> => |s| s.as_slice();
    impl[T: bytemuck::Pod] WritablePixelArray<T> for alloc::vec::Vec<T> => |s| s.as_mut_slice();
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
    fn pack(pixels: &[BinaryColor]) -> alloc::vec::Vec<u8> {
        let mut buf = BinaryColor::packed_vec(pixels.len());
        for (i, &p) in pixels.iter().enumerate() {
            if p.is_on() {
                WritablePixelArray::<BinaryColor>::write(&mut buf, i, p);
            }
        }
        buf
    }
}

impl WritablePixelArray<BinaryColor> for [u8] {
    #[inline]
    fn write(&mut self, idx: usize, value: BinaryColor) {
        if value.is_on() {
            self[idx / 8] |= 1 << (idx % 8);
        } else {
            self[idx / 8] &= !(1 << (idx % 8));
        }
    }
}

#[cfg(feature = "alloc")]
forward_pixel_array! {
    impl PixelArray<BinaryColor> for alloc::vec::Vec<u8> => |s| s.as_slice();
    impl WritablePixelArray<BinaryColor> for alloc::vec::Vec<u8> => |s| s.as_mut_slice();
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: BinaryColor = BinaryColor::ON;
    const OFF: BinaryColor = BinaryColor::OFF;

    fn read_bit(buf: &[u8], idx: usize) -> BinaryColor {
        PixelArray::<BinaryColor>::read(buf, idx)
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
    fn slice_write_from_stops_at_end() {
        let mut buf = [0u8; 4];
        WritablePixelArray::<u8>::write_from(&mut buf[..], 2, core::iter::repeat(7));
        assert_eq!(buf, [0, 0, 7, 7]);
    }

    #[test]
    fn slice_update_from_passes_source_index_dst_and_src() {
        let mut buf = [10u8, 20, 30, 40];
        WritablePixelArray::<u8>::update_from(&mut buf[..], 1, [1, 2], |i, dst, src| {
            dst + src + i as u8 * 100
        });
        assert_eq!(buf, [10, 21, 132, 40]);
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
    fn packed_bits_are_lsb_first_and_independent() {
        let mut buf = [0u8; 2];
        WritablePixelArray::write(&mut buf[..], 1, ON);
        WritablePixelArray::write(&mut buf[..], 9, ON);
        assert_eq!(buf, [0b0000_0010, 0b0000_0010]);
        assert_eq!(read_bit(&buf, 1), ON);
        assert_eq!(read_bit(&buf, 2), OFF);

        WritablePixelArray::write(&mut buf[..], 1, OFF);
        assert_eq!(buf, [0, 0b0000_0010]);
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

    #[test]
    fn packed_write_from_stops_at_end_when_start_is_nonzero() {
        let mut buf = [0u8; 1];
        WritablePixelArray::write_from(&mut buf[..], 3, core::iter::repeat(ON));
        assert_eq!(buf, [0b1111_1000]);
    }

    #[test]
    fn packed_update_from_stops_at_end_when_start_is_nonzero() {
        let mut buf = [0u8; 1];
        WritablePixelArray::update_from(&mut buf[..], 6, [ON, ON, ON, ON], |_, _, src| src);
        assert_eq!(buf, [0b1100_0000]);
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn packed_vec_rounds_up_to_whole_bytes() {
        for (len, bytes) in [(0, 0), (1, 1), (8, 1), (9, 2), (16, 2), (17, 3)] {
            assert_eq!(BinaryColor::packed_vec(len).len(), bytes, "{len} pixels");
        }
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn packed_vec_can_address_every_pixel() {
        let mut buf = BinaryColor::packed_vec(9);
        WritablePixelArray::write(&mut buf, 8, ON);
        assert_eq!(PixelArray::<BinaryColor>::read(&buf, 8), ON);
        assert_eq!(PixelArray::<BinaryColor>::max_len(&buf), 16);
        assert_eq!(
            PixelArray::<BinaryColor>::pixels_from(&buf, 8).next(),
            Some(ON)
        );
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn vec_delegates_to_slice_behaviour() {
        let mut buf = alloc::vec![1u8, 2, 3];
        WritablePixelArray::<u8>::write_from(&mut buf, 1, core::iter::repeat(9));
        assert_eq!(buf, [1, 9, 9]);
        WritablePixelArray::<u8>::update_from(&mut buf, 0, [1], |_, dst, src| dst + src);
        assert_eq!(PixelArray::<u8>::read(&buf, 0), 2);
    }
}
