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

            rgb!(@colors $ty, $pixel, black, red, green, blue, cyan, magenta, yellow, white);

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

            impl From<&$pixel> for $ty {
                fn from(value: &$pixel) -> Self {
                    Self(value.[<to_ $endian _bytes>]())
                }
            }

            impl From<$pixel> for $ty {
                fn from(value: $pixel) -> Self {
                    $ty::from(&value)
                }
            }

            impl From<$ty> for Rgb888 {
                fn from(value: $ty) -> Self {
                    pixelcolor::Rgb888::from($pixel::from(value)).into()
                }
            }

            impl Pixel for $ty {
                type Array = [Self];

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
    (@colors $ty:ident, $pixel:ty, $($color:ident),+) => {
        pastey::paste! {
            impl $ty {
                $(pub fn $color() -> Self {
                    $pixel::[<$color:upper>].into()
                })+
            }
        }
    };
}

rgb!(Rgb555, Bgr555, Rgb565, Bgr565, Rgb666, Bgr666);

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

#[cfg(test)]
mod tests {
    use embedded_graphics_core::pixelcolor::{self as eg, RgbColor};

    use super::*;

    #[test]
    fn mix_returns_exact_endpoints() {
        for dst in 0..=255 {
            for src in 0..=255 {
                assert_eq!(mix(dst, src, 0), dst);
                assert_eq!(mix(dst, src, 255), src);
            }
        }
    }

    #[test]
    fn mix_rounds_to_nearest() {
        assert_eq!(mix(0, 255, 128), 128);
        assert_eq!(mix(255, 0, 128), 127);
        assert_eq!(mix(0, 31, 128), 16);
        assert_eq!(mix(31, 31, 77), 31);
    }

    macro_rules! round_trip_tests {
        ($($ty:ident, $eg:ident, $format:literal);+ $(;)?) => {
            $(pastey::paste! {
                #[test]
                fn [<$ty:lower _round_trips>]() {
                    let (r, g, b) = (eg::$eg::MAX_R, eg::$eg::MAX_G / 2, 1);
                    let px = $ty::new(r, g, b);
                    assert_eq!((px.r(), px.g(), px.b()), (r, g, b));

                    let color = eg::$eg::from(px);
                    assert_eq!((color.r(), color.g(), color.b()), (r, g, b));
                    assert_eq!($ty::from(color), px);
                    assert_eq!($ty::from(&color), px);
                }

                #[test]
                fn [<$ty:lower _named_colors_and_8_bit_input>]() {
                    assert_eq!($ty::from_rgb8(0, 0, 0), $ty::black());
                    assert_eq!($ty::from_rgb8(255, 255, 255), $ty::white());
                    assert_eq!($ty::from_rgb8(255, 0, 0), $ty::red());
                    assert_eq!($ty::from(Rgb888::new(0, 0, 255)), $ty::blue());
                    assert_eq!(Rgb888::from($ty::white()), Rgb888::new(255, 255, 255));
                }

                #[test]
                fn [<$ty:lower _blend_endpoints>]() {
                    assert_eq!($ty::red().blend($ty::blue(), 0), $ty::red());
                    assert_eq!($ty::red().blend($ty::blue(), 255), $ty::blue());
                }

                #[test]
                fn [<$ty:lower _format_name>]() {
                    assert_eq!(<$ty as Pixel>::FORMAT, $format);
                }
            })+
        };
    }

    round_trip_tests! {
        Rgb555Le, Rgb555, "rgb555le";
        Rgb555Be, Rgb555, "rgb555be";
        Bgr555Le, Bgr555, "bgr555le";
        Bgr555Be, Bgr555, "bgr555be";
        Rgb565Le, Rgb565, "rgb565le";
        Rgb565Be, Rgb565, "rgb565be";
        Bgr565Le, Bgr565, "bgr565le";
        Bgr565Be, Bgr565, "bgr565be";
        Rgb666Le, Rgb666, "rgb666le";
        Rgb666Be, Rgb666, "rgb666be";
        Bgr666Le, Bgr666, "bgr666le";
        Bgr666Be, Bgr666, "bgr666be";
    }

    #[test]
    fn wire_format_byte_order() {
        // Bytes are hand-derived from each format's bit layout, with only red set.
        assert_eq!(bytemuck::bytes_of(&Rgb555Le::red()), [0x00, 0x7C]);
        assert_eq!(bytemuck::bytes_of(&Rgb555Be::red()), [0x7C, 0x00]);
        assert_eq!(bytemuck::bytes_of(&Rgb565Le::red()), [0x00, 0xF8]);
        assert_eq!(bytemuck::bytes_of(&Rgb565Be::red()), [0xF8, 0x00]);
        assert_eq!(bytemuck::bytes_of(&Bgr565Le::red()), [0x1F, 0x00]);
        assert_eq!(bytemuck::bytes_of(&Bgr565Be::red()), [0x00, 0x1F]);
        assert_eq!(bytemuck::bytes_of(&Rgb666Le::red()), [0x00, 0xF0, 0x03]);
        assert_eq!(bytemuck::bytes_of(&Rgb666Be::red()), [0x03, 0xF0, 0x00]);
    }

    #[test]
    fn rgb888_converts_both_ways() {
        let ours = Rgb888::new(1, 2, 3);
        let theirs = eg::Rgb888::from(ours);
        assert_eq!((theirs.r(), theirs.g(), theirs.b()), (1, 2, 3));
        assert_eq!(Rgb888::from(theirs), ours);
    }

    #[test]
    fn binary_color_converts_both_ways() {
        assert_eq!(eg::BinaryColor::from(BinaryColor::ON), eg::BinaryColor::On);
        assert_eq!(BinaryColor::from(eg::BinaryColor::Off), BinaryColor::OFF);
    }
}
