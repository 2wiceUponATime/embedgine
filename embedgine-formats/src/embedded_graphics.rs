use core::fmt::Debug;

use embedded_graphics_core::pixelcolor::{
    self, PixelColor, RgbColor,
    raw::{RawData, ToBytes},
};

use super::*;

fn mix(dst: u8, src: u8, alpha: u8) -> u8 {
    ((dst as u16 * (255 - alpha) as u16 + src as u16 * alpha as u16 + 127) / 255) as u8
}

impl From<Rgb888> for pixelcolor::Rgb888 {
    fn from(Rgb888 { r, g, b }: Rgb888) -> Self {
        Self::new(r, g, b)
    }
}

impl From<pixelcolor::Rgb888> for Rgb888 {
    fn from(color: pixelcolor::Rgb888) -> Self {
        Self::new(color.r(), color.g(), color.b())
    }
}

macro_rules! rgb {
        ($($ty:ident),* $(,)?) => {
            $(pastey::paste! {
                rgb!(@endian [<$ty Le>], pixelcolor::$ty, le);
                rgb!(@endian [<$ty Be>], pixelcolor::$ty, be);
            })*
        };
        (@endian $ty:ident, $pixel:ty, $endian:ident) => {
            pastey::paste! {
                #[derive(Clone, Copy, Default, PartialEq, Eq, bytemuck::Zeroable, bytemuck::Pod)]
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

                    pub fn r(&self) -> u8 {
                        $pixel::from(self).r()
                    }

                    pub fn g(&self) -> u8 {
                        $pixel::from(self).g()
                    }

                    pub fn b(&self) -> u8 {
                        $pixel::from(self).b()
                    }
                }

                impl Debug for $ty {
                    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                        write!(f, "{}({}, {}, {})", stringify!([<$ty:lower>]), self.r(), self.g(), self.b())
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

                impl From<$ty> for Rgb888 {
                    fn from(value: $ty) -> Self {
                        pixelcolor::Rgb888::from($pixel::from(value)).into()
                    }
                }

                impl Pixel for $ty {
                    const FORMAT: &'static str = stringify!([<$ty:lower>]);
                }

                impl Blend for $ty {
                    fn blend(self, src: Self, alpha: u8) -> Self {
                        Self::new(
                            mix(self.r(), src.r(), alpha),
                            mix(self.g(), src.g(), alpha),
                            mix(self.b(), src.b(), alpha),
                        )
                    }
                }
            }
        };
    }

rgb!(Rgb555, Bgr555, Rgb565, Bgr565, Rgb666, Bgr666);

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

impl From<pixelcolor::BinaryColor> for BinaryColor {
    fn from(value: pixelcolor::BinaryColor) -> Self {
        value.is_on().into()
    }
}

impl From<BinaryColor> for pixelcolor::BinaryColor {
    fn from(value: BinaryColor) -> Self {
        value.is_on().into()
    }
}

impl Pixel for BinaryColor {
    const FORMAT: &'static str = "binary";
}
