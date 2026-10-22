#![no_std]

use core::fmt::{Debug, Display};

#[cfg(feature = "embedded-graphics")]
use embedded_graphics::pixelcolor::{
    self, PixelColor, RgbColor,
    raw::{RawData, ToBytes},
};

mod sealed {
    pub trait Sealed {}
}

macro_rules! impl_fmt_tuple {
    ($ty:ident => $name:ident ( $($field:ident),* $(,)? )) => {
        impl Debug for $ty {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.debug_tuple(stringify!($name))
                    $(.field(&self.$field))*
                    .finish()
            }
        }

        impl Display for $ty {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{}(", stringify!($name))?;
                impl_fmt_tuple!(@write f, $(&self.$field),*);
                write!(f, ")")?;
                Ok(())
            }
        }
    };

    (@write $f:expr,) => {};
    (@write $f:expr, $first:expr $(, $rest:expr)*) => {
        write!($f, "{}", $first)?;
        $(write!($f, ", {}", $rest)?;)*
    }
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

impl_fmt_tuple!(Rgba8 => rgba8(r, g, b, a));

pub trait Pixel: Debug + Display + Copy + Default + bytemuck::Pod + From<Rgb888> {
    const FORMAT: &'static str;
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

impl_fmt_tuple!(Rgb888 => rgb888(r, g, b));

impl From<Rgba8> for Rgb888 {
    fn from(Rgba8 { r, g, b, .. }: Rgba8) -> Self {
        Self { r, g, b }
    }
}

impl Pixel for Rgb888 {
    const FORMAT: &'static str = "rgb888";
}

#[cfg(feature = "embedded-graphics")]
impl From<Rgb888> for pixelcolor::Rgb888 {
    fn from(Rgb888 { r, g, b }: Rgb888) -> Self {
        Self::new(r, g, b)
    }
}

macro_rules! embedded_pixels {
    ($($ty:ident),* $(,)?) => {
        $(pastey::paste! {
            embedded_pixels!(@endian [<$ty Le>], pixelcolor::$ty, le);
            embedded_pixels!(@endian [<$ty Be>], pixelcolor::$ty, be);
        })*
    };
    (@endian $ty:ident, $pixel:ty, $endian:ident) => {
        pastey::paste! {
            #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, bytemuck::Zeroable, bytemuck::Pod)]
            #[repr(transparent)]
            pub struct $ty(<$pixel as ToBytes>::Bytes);

            impl $ty {
                pub fn new(r: u8, g: u8, b: u8) -> Self {
                    Self($pixel::new(r, g, b).[<to_ $endian _bytes>]())
                }

                pub fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
                    let inner: $pixel = pixelcolor::Rgb888::new(r, g, b).into();
                    Self(inner.[<to_ $endian _bytes>]())
                }
            }

            impl Display for $ty {
                fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    let color: $pixel = self.into();
                    write!(f, "rgb({}, {}, {})", color.r(), color.g(), color.b())
                }
            }

            impl From<Rgb888> for $ty {
                fn from(Rgb888 { r, g, b }: Rgb888) -> Self {
                    Self::from_rgb8(r, g, b)
                }
            }

            impl From<&$ty> for $pixel {
                fn from(value: &$ty) -> Self {
                    type Raw = <$pixel as PixelColor>::Raw;
                    type Storage = <Raw as RawData>::Storage;
                    const STORAGE_SIZE: usize = core::mem::size_of::<Storage>();
                    let mut bytes = [0u8; STORAGE_SIZE];
                    match stringify!($endian) {
                        "le" => &mut bytes[..value.0.len()],
                        "be" => &mut bytes[STORAGE_SIZE - value.0.len()..],
                        _ => unreachable!(),
                    }
                    .copy_from_slice(&value.0);
                    $pixel::from(Raw::new(Storage::[<from_ $endian _bytes>](bytes)))
                }
            }

            impl From<$ty> for $pixel {
                fn from(value: $ty) -> Self {
                    $pixel::from(&value)
                }
            }

            impl Pixel for $ty {
                const FORMAT: &'static str = stringify!([<$ty:lower>]);
            }
        }
    };
}

#[cfg(feature = "embedded-graphics")]
embedded_pixels!(Rgb555, Bgr555, Rgb565, Bgr565, Rgb666, Bgr666);
