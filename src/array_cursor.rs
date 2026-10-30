use core::range::Range;

use crate::range_ext::RangeExt;

pub struct ArrayCursor {
    pub index: u32,
}

impl ArrayCursor {
    pub fn new(index: u32) -> Self {
        Self { index }
    }

    pub fn step(&mut self, step: u16) -> Range<usize> {
        let start = self.index;
        self.index += step as u32;
        Range::from(start..self.index).cast()
    }
}
