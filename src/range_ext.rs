use core::{ops::Sub, range::Range};

use num_traits::AsPrimitive;

mod sealed {
    pub trait Sealed {}
}

pub trait RangeExt<T>: sealed::Sealed {
    fn len(self) -> T::Output
    where
        T: Sub;

    fn map_ends<U>(self, f: impl FnMut(T) -> U) -> Range<U>;

    fn cast<U: Copy + 'static>(self) -> Range<U>
    where
        T: AsPrimitive<U>;

    fn intersection(self, other: Range<T>) -> Option<Range<T>>
    where
        T: Ord;

    fn sub_from(self, value: T) -> Range<T::Output>
    where
        T: Copy + Sub;

    fn sub<U: Copy>(self, value: U) -> Range<T::Output>
    where
        T: Sub<U>;
}

impl<T> sealed::Sealed for Range<T> {}

impl<T> RangeExt<T> for Range<T> {
    fn len(self) -> T::Output
    where
        T: Sub,
    {
        self.end - self.start
    }

    fn map_ends<U>(self, mut f: impl FnMut(T) -> U) -> Range<U> {
        (f(self.start)..f(self.end)).into()
    }

    fn cast<U: Copy + 'static>(self) -> Range<U>
    where
        T: AsPrimitive<U>,
    {
        self.map_ends(|v| v.as_())
    }

    fn intersection(self, other: Range<T>) -> Option<Range<T>>
    where
        T: Ord,
    {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);

        if start < end {
            Some((start..end).into())
        } else {
            None
        }
    }

    fn sub_from(self, value: T) -> Range<T::Output>
    where
        T: Copy + Sub,
    {
        (value - self.end..value - self.start).into()
    }

    fn sub<U: Copy>(self, value: U) -> Range<T::Output>
    where
        T: Sub<U>,
    {
        self.map_ends(|e| e - value)
    }
}
