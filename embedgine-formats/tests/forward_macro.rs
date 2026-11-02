//! `forward_pixel_array!` must expand in a crate that imports nothing from `embedgine_formats`.

#![no_implicit_prelude]

pub struct Wrapper<'a>(pub ::embedgine_formats::ArrayBuffer<&'a mut [u8]>);

::embedgine_formats::forward_pixel_array! {
    impl['a] PixelBuffer<u8> for Wrapper<'a> => |s| s.0;
}

#[test]
fn forwarded_buffer_delegates_to_inner() {
    use ::embedgine_formats::{Dir, PixelBuffer, geometry::Vec2};

    let mut storage = [0u8; 4];
    let mut wrapper = Wrapper(::embedgine_formats::ArrayBuffer::new(&mut storage[..], 2));
    ::core::assert_eq!(wrapper.capacity(), Vec2::new(2, 2));
    wrapper.write_from(Vec2::new(0, 1), Dir::X, [1, 2]);
    wrapper.update_from(Vec2::new(1, 1), Dir::YRev, [10, 10], |_, dst, src| {
        dst + src
    });
    ::core::assert_eq!(wrapper.read_pos(Vec2::new(1, 0)), 10);
    ::core::assert_eq!(storage, [0, 10, 1, 12]);
}
