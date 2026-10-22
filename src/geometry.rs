use core::{
    num::TryFromIntError,
    ops::{Add, Mul, Sub},
};

#[cfg(feature = "embedded-graphics")]
use embedded_graphics::{
    geometry::{Point, Size},
    primitives::Rectangle,
};

mod private {
    pub trait Sealed {}
}

macro_rules! scalars {
    ($($ty:ident),+ $(,)?) => {
        pub trait Scalar: Copy {}
        $(impl Scalar for $ty {})+
    };
}

scalars!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64,
);

macro_rules! int_widths {
    ($($ty:ident($signed:ident, $unsigned:ident $(,)?)),+ $(,)?) => {
        pub trait IntWidth: private::Sealed {
            type Signed: TryFrom<Self::Unsigned, Error = TryFromIntError> + Copy;
            type Unsigned: TryFrom<Self::Signed, Error = TryFromIntError> + Copy;

            const SIGNED_MAX: Self::Signed;

            fn to_signed(s: Self::Unsigned) -> Self::Signed;
            fn to_unsigned(s: Self::Signed) -> Self::Unsigned;
        }
        $(
            #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub struct $ty;
            impl private::Sealed for $ty {}
            impl IntWidth for $ty {
                type Signed = $signed;
                type Unsigned = $unsigned;

                const SIGNED_MAX: Self::Signed = Self::Signed::MAX;

                #[inline]
                fn to_signed(s: Self::Unsigned) -> Self::Signed {
                    s as Self::Signed
                }

                #[inline]
                fn to_unsigned(s: Self::Signed) -> Self::Unsigned {
                    s as Self::Unsigned
                }
            }
        )+
    };
}

int_widths!(
    W8(i8, u8),
    W16(i16, u16),
    W32(i32, u32),
    W64(i64, u64),
    W128(i128, u128),
);

macro_rules! impl_vec2_ops {
    {
        binary ( $($binary:ident),+ $(,)? ),
        unary ( $($unary:ident),+ $(,)? ) $(,)?
    } => {
        $(pastey::paste! {
            impl<T, U: core::ops::$binary<T>> core::ops::$binary<Vec2<T>> for Vec2<U> {
                type Output = Vec2<<U as core::ops::$binary<T>>::Output>;

                fn [<$binary:lower>](self, rhs: Vec2<T>) -> Self::Output {
                    self.zip_with(rhs, |a, b| a.[<$binary:lower>](b))
                }
            }

            impl<T, U: core::ops::[<$binary Assign>]<T>> core::ops::[<$binary Assign>]<Vec2<T>> for Vec2<U> {
                fn [<$binary:lower _assign>](&mut self, rhs: Vec2<T>) {
                    self.as_mut().zip_with(rhs, |a, b| a.[<$binary:lower _assign>](b));
                }
            }

            impl<T: Scalar, U: core::ops::$binary<T>> core::ops::$binary<T> for Vec2<U> {
                type Output = Vec2<<U as core::ops::$binary<T>>::Output>;

                fn [<$binary:lower>](self, rhs: T) -> Self::Output {
                    self.map(|v| v.[<$binary:lower>](rhs))
                }
            }

            impl<T: Scalar, U: core::ops::[<$binary Assign>]<T>> core::ops::[<$binary Assign>]<T> for Vec2<U> {
                fn [<$binary:lower _assign>](&mut self, rhs: T) {
                    self.as_mut().map(|v| v.[<$binary:lower _assign>](rhs));
                }
            }
        })+
        $(pastey::paste! {
            impl<T: core::ops::$unary> core::ops::$unary for Vec2<T> {
                type Output = Vec2<<T as core::ops::$unary>::Output>;

                fn [<$unary:lower>](self) -> Self::Output {
                    self.map(|v| v.[<$unary:lower>]())
                }
            }
        })+
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Vec2<T> {
    pub x: T,
    pub y: T,
}

impl<T> Vec2<T> {
    pub fn new(x: T, y: T) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn zip_with<U, R>(self, other: Vec2<U>, mut f: impl FnMut(T, U) -> R) -> Vec2<R> {
        Vec2 {
            x: f(self.x, other.x),
            y: f(self.y, other.y),
        }
    }

    #[inline]
    pub fn zip<U>(self, other: Vec2<U>) -> Vec2<(T, U)> {
        self.zip_with(other, |a, b| (a, b))
    }

    #[inline]
    pub fn map<U>(self, mut f: impl FnMut(T) -> U) -> Vec2<U> {
        Vec2 {
            x: f(self.x),
            y: f(self.y),
        }
    }

    #[inline]
    pub fn cast<U: From<T>>(self) -> Vec2<U> {
        self.map(|v| v.into())
    }

    #[inline]
    pub fn try_cast<U: TryFrom<T>>(self) -> Result<Vec2<U>, U::Error> {
        self.map(|v| v.try_into()).transpose()
    }

    #[inline]
    pub fn as_ref(&self) -> Vec2<&T> {
        Vec2 {
            x: &self.x,
            y: &self.y,
        }
    }

    #[inline]
    pub fn as_mut(&mut self) -> Vec2<&mut T> {
        Vec2 {
            x: &mut self.x,
            y: &mut self.y,
        }
    }
}

impl<T, E> Vec2<Result<T, E>> {
    pub fn transpose(self) -> Result<Vec2<T>, E> {
        Ok(Vec2 {
            x: self.x?,
            y: self.y?,
        })
    }
}

impl<T> Vec2<Option<T>> {
    pub fn transpose(self) -> Option<Vec2<T>> {
        Some(Vec2 {
            x: self.x?,
            y: self.y?,
        })
    }
}

#[cfg(feature = "embedded-graphics")]
impl From<Point> for Vec2<i32> {
    fn from(value: Point) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

#[cfg(feature = "embedded-graphics")]
impl From<Vec2<i32>> for Point {
    fn from(value: Vec2<i32>) -> Self {
        Self::new(value.x, value.y)
    }
}

#[cfg(feature = "embedded-graphics")]
impl From<Size> for Vec2<u32> {
    fn from(value: Size) -> Self {
        Self {
            x: value.width,
            y: value.height,
        }
    }
}

#[cfg(feature = "embedded-graphics")]
impl From<Vec2<u32>> for Size {
    fn from(value: Vec2<u32>) -> Self {
        Self::new(value.x, value.y)
    }
}

impl_vec2_ops! {
    binary(Add, Sub, Mul, Div, Rem, BitAnd, BitOr, BitXor, Shl, Shr),
    unary(Neg, Not),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect<T: IntWidth> {
    pub pos: Vec2<T::Signed>,
    pub size: Vec2<T::Unsigned>,
}

impl<T: IntWidth> Rect<T> {
    pub fn new(pos: Vec2<T::Signed>, size: Vec2<T::Unsigned>) -> Self
    where
        T::Unsigned: PartialOrd,
    {
        assert!(
            size.x <= T::to_unsigned(T::SIGNED_MAX) && size.y <= T::to_unsigned(T::SIGNED_MAX),
            "size greater than unsigned integer max",
        );
        Self { pos, size }
    }

    pub fn from_corners(a: Vec2<T::Signed>, b: Vec2<T::Signed>) -> Self
    where
        T::Signed: Copy + Sub<Output = T::Signed> + Ord,
        T::Unsigned: PartialOrd,
    {
        let pos = a.zip_with(b, T::Signed::min);
        let size = a.zip_with(b, T::Signed::max) - pos;
        Self::new(pos, size.try_cast().unwrap())
    }

    pub fn cast<U: IntWidth>(self) -> Rect<U>
    where
        U::Signed: From<T::Signed>,
        U::Unsigned: From<T::Unsigned>,
    {
        Rect {
            pos: self.pos.cast(),
            size: self.size.cast(),
        }
    }

    pub fn width(&self) -> T::Unsigned {
        self.size.x
    }

    pub fn height(&self) -> T::Unsigned {
        self.size.y
    }

    pub fn dimensions(&self) -> (T::Unsigned, T::Unsigned) {
        (self.width(), self.height())
    }

    pub fn area(&self) -> <T::Unsigned as Mul>::Output
    where
        T::Unsigned: Mul,
    {
        self.width() * self.height()
    }

    pub fn left(&self) -> T::Signed {
        self.pos.x
    }

    pub fn right(&self) -> <T::Signed as Add>::Output
    where
        T::Signed: Add,
    {
        self.pos.x + T::to_signed(self.size.x)
    }

    pub fn top(&self) -> T::Signed {
        self.pos.y
    }

    pub fn bottom(&self) -> <T::Signed as Add>::Output
    where
        T::Signed: Add,
    {
        self.pos.y + T::to_signed(self.size.y)
    }
}

#[cfg(feature = "embedded-graphics")]
impl From<Rect<W32>> for Rectangle {
    fn from(value: Rect<W32>) -> Self {
        Self::new(value.pos.into(), value.size.into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct URect<T: IntWidth> {
    pub pos: Vec2<T::Unsigned>,
    pub size: Vec2<T::Unsigned>,
}

impl<T: IntWidth> URect<T> {
    pub fn new(pos: Vec2<T::Unsigned>, size: Vec2<T::Unsigned>) -> Self {
        Self { pos, size }
    }

    pub fn from_corners(a: Vec2<T::Unsigned>, b: Vec2<T::Unsigned>) -> Self
    where
        T::Unsigned: Ord + Sub<Output = T::Unsigned>,
    {
        let pos = a.zip_with(b, T::Unsigned::min);
        let size = a.zip_with(b, T::Unsigned::max) - pos;
        URect::new(pos, size)
    }

    pub fn cast<U: IntWidth>(self) -> URect<U>
    where
        U::Unsigned: From<T::Unsigned>,
    {
        URect {
            pos: self.pos.cast(),
            size: self.size.cast(),
        }
    }

    pub fn width(&self) -> T::Unsigned {
        self.size.x
    }

    pub fn height(&self) -> T::Unsigned {
        self.size.y
    }

    pub fn dimensions(&self) -> (T::Unsigned, T::Unsigned) {
        (self.width(), self.height())
    }

    pub fn area(&self) -> <T::Unsigned as Mul>::Output
    where
        T::Unsigned: Mul,
    {
        self.width() * self.height()
    }

    pub fn left(&self) -> T::Unsigned {
        self.pos.x
    }

    pub fn right(&self) -> <T::Unsigned as Add>::Output
    where
        T::Unsigned: Add,
    {
        self.pos.x + self.size.x
    }

    pub fn top(&self) -> T::Unsigned {
        self.pos.y
    }

    pub fn bottom(&self) -> <T::Unsigned as Add>::Output
    where
        T::Unsigned: Add,
    {
        self.pos.y + self.size.y
    }
}

#[cfg(feature = "embedded-graphics")]
impl From<URect<W32>> for Rectangle {
    fn from(value: URect<W32>) -> Self {
        Self::new(value.pos.map(|v| v as i32).into(), value.size.into())
    }
}
