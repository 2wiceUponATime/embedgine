#![no_std]

use core::fmt::Debug;

#[cfg(feature = "embedded-graphics")]
pub mod embedded_graphics;

mod sealed {
    pub trait Sealed {}
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

pub trait Pixel: Debug + Copy + Default + bytemuck::Pod + From<Rgb888> {
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
    const FORMAT: &'static str = "rgb888";
}
