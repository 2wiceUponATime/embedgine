use core::ops::{Range, Sub};

mod sealed {
    pub trait Sealed {}
}

pub trait RangeExt<T>: sealed::Sealed {
    fn map_ends<U>(self, f: impl FnMut(T) -> U) -> Range<U>;

    fn intersection(self, other: Range<T>) -> Option<Range<T>>
    where
        T: Ord;

    fn sub_from(self, value: T) -> Range<T::Output>
    where
        T: Clone + Sub;
}

impl<T> sealed::Sealed for Range<T> {}

impl<T> RangeExt<T> for Range<T> {
    fn map_ends<U>(self, mut f: impl FnMut(T) -> U) -> Range<U> {
        f(self.start)..f(self.end)
    }

    fn intersection(self, other: Range<T>) -> Option<Range<T>>
    where
        T: Ord,
    {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);

        if start < end { Some(start..end) } else { None }
    }

    fn sub_from(self, value: T) -> Range<<T>::Output>
    where
        T: Clone + Sub,
    {
        value.clone() - self.end..value - self.start
    }
}
