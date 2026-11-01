use alloc::{vec, vec::Vec};
use core::{
    future::Future,
    pin::{Pin, pin},
    task::{Context, Poll, Waker},
};

use embedded_graphics_core::{
    Pixel as EgPixel,
    geometry::{OriginDimensions, Size},
    pixelcolor::Rgb565,
};

use super::*;
use crate::{
    formats::{BinaryColor, embedded_graphics::Rgb565Le as Px},
    geometry::Rect,
};

const TRANSFORMS: [Transform; 8] = [
    Transform::Identity,
    Transform::Transpose,
    Transform::FlipY,
    Transform::RotCcw,
    Transform::FlipX,
    Transform::RotCw,
    Transform::Rot180,
    Transform::Transverse,
];

fn bg() -> Px {
    Px::white()
}

/// A distinct, non-white color for each index.
fn color(i: u8) -> Px {
    Px::new(i % 32, i / 2 % 64, 0)
}

/// Where a source pixel lands, written out per transform so that it shares no
/// code with `Transform`'s flip and transpose bits.
fn place(t: Transform, (x, y): (i16, i16), (w, h): (i16, i16)) -> (i16, i16) {
    match t {
        Transform::Identity => (x, y),
        Transform::Transpose => (y, x),
        Transform::FlipY => (x, h - 1 - y),
        Transform::RotCcw => (y, w - 1 - x),
        Transform::FlipX => (w - 1 - x, y),
        Transform::RotCw => (h - 1 - y, x),
        Transform::Rot180 => (w - 1 - x, h - 1 - y),
        Transform::Transverse => (h - 1 - y, w - 1 - x),
    }
}

fn placed_size(t: Transform, (w, h): (i16, i16)) -> (i16, i16) {
    match t {
        Transform::Identity | Transform::FlipX | Transform::FlipY | Transform::Rot180 => (w, h),
        _ => (h, w),
    }
}

/// Owns the encoded form of an image described pixel by pixel.
struct Sprite {
    width: u16,
    source: Vec<Vec<(Px, u8)>>,
    pixels: Vec<Px>,
    alphas: Vec<u8>,
    spans: Vec<Span>,
    row_starts: Vec<RowStart>,
}

impl Sprite {
    fn new(source: Vec<Vec<(Px, u8)>>) -> Self {
        let mut sprite = Self {
            width: source[0].len() as u16,
            source: Vec::new(),
            pixels: Vec::new(),
            alphas: Vec::new(),
            spans: Vec::new(),
            row_starts: Vec::new(),
        };
        for row in &source {
            sprite.row_starts.push(RowStart {
                px_idx: sprite.pixels.len() as u32,
                alpha_idx: sprite.alphas.len() as u32,
                span_idx: sprite.spans.len() as u16,
            });
            let mut previous = None;
            for &(px, alpha) in row {
                let kind = SpanKind::from_alpha(alpha);
                if previous == Some(kind) {
                    sprite.spans.last_mut().unwrap().len += 1;
                } else {
                    sprite.spans.push(Span { len: 1, kind });
                }
                previous = Some(kind);
                if kind != SpanKind::Transparent {
                    sprite.pixels.push(px);
                }
                if kind == SpanKind::Blended {
                    sprite.alphas.push(alpha);
                }
            }
        }
        sprite.source = source;
        sprite
    }

    /// Builds a sprite from rows of `(color index, alpha)` values.
    fn from_rows(rows: &[&[(u8, u8)]]) -> Self {
        Self::new(
            rows.iter()
                .map(|row| row.iter().map(|&(c, a)| (color(c), a)).collect())
                .collect(),
        )
    }

    fn image<K: AlphaKind>(&self) -> Image<'_, Px, K> {
        Image::new(
            self.width,
            self.pixels.as_slice(),
            &self.alphas,
            &self.spans,
            &self.row_starts,
        )
    }

    fn size(&self) -> (i16, i16) {
        (self.width as i16, self.source.len() as i16)
    }
}

fn opaque_sprite() -> Sprite {
    Sprite::from_rows(&[
        &[(1, 255), (2, 255), (3, 255), (4, 255)],
        &[(5, 255), (6, 255), (7, 255), (8, 255)],
        &[(9, 255), (10, 255), (11, 255), (12, 255)],
    ])
}

/// Transparent spans at the start, middle and end of rows, and a fully
/// transparent row.
fn masked_sprite() -> Sprite {
    Sprite::from_rows(&[
        &[(0, 0), (2, 255), (3, 255), (0, 0)],
        &[(5, 255), (0, 0), (7, 255), (8, 255)],
        &[(0, 0), (0, 0), (0, 0), (0, 0)],
    ])
}

/// All span kinds, with varying alphas inside one blended span.
fn blended_sprite() -> Sprite {
    Sprite::from_rows(&[
        &[(1, 40), (2, 128), (3, 255), (0, 0)],
        &[(0, 0), (6, 255), (7, 200), (8, 90)],
        &[(9, 255), (0, 0), (11, 10), (12, 255)],
    ])
}

/// Naive per-pixel rendering of `sprite` onto a full frame.
fn reference_frame(
    sprite: &Sprite,
    (w, h): (u16, u16),
    pos: Vec2<i16>,
    transform: Transform,
) -> Vec<Px> {
    let mut frame = vec![bg(); w as usize * h as usize];
    for (sy, row) in sprite.source.iter().enumerate() {
        for (sx, &(px, alpha)) in row.iter().enumerate() {
            let (x, y) = place(transform, (sx as i16, sy as i16), sprite.size());
            let (x, y) = (x + pos.x, y + pos.y);
            if x < 0 || y < 0 || x >= w as i16 || y >= h as i16 {
                continue;
            }
            let dst = &mut frame[y as usize * w as usize + x as usize];
            *dst = match alpha {
                0 => *dst,
                255 => px,
                _ => dst.blend(px, alpha),
            };
        }
    }
    frame
}

/// Draws one band at a time into a band-sized buffer and stitches the results.
fn banded_frame<K: DrawAlpha<Px>>(
    image: &Image<'_, Px, K>,
    (w, h): (u16, u16),
    band_height: u16,
    pos: Vec2<i16>,
    transform: Transform,
) -> Vec<Px> {
    let mut frame = Vec::with_capacity(w as usize * h as usize);
    for band in Renderer::<Px, Vec<Px>>::bands(h, band_height) {
        let len = w as usize * band.len() as usize;
        let mut canvas = Canvas::new(Vec2::new(w, h), band, vec![bg(); len]);
        canvas.draw_image(image, pos, transform);
        for y in band.start..band.end {
            frame.extend((0..w).map(|x| canvas.read(Vec2::new(x, y))));
        }
    }
    frame
}

fn assert_matches_reference<K: DrawAlpha<Px>>(sprite: &Sprite) {
    const SIZE: (u16, u16) = (7, 6);
    let image = sprite.image::<K>();
    for transform in TRANSFORMS {
        for band_height in [1, 2, 4, 6] {
            for y in -5..=7 {
                for x in -5..=8 {
                    let pos = Vec2::new(x, y);
                    assert_eq!(
                        banded_frame(&image, SIZE, band_height, pos, transform),
                        reference_frame(sprite, SIZE, pos, transform),
                        "{transform:?}, band height {band_height}, position ({x}, {y})",
                    );
                }
            }
        }
    }
}

#[test]
fn opaque_matches_reference_renderer() {
    assert_matches_reference::<Opaque>(&opaque_sprite());
}

#[test]
fn masked_matches_reference_renderer() {
    assert_matches_reference::<Masked>(&masked_sprite());
}

#[test]
fn blended_matches_reference_renderer() {
    assert_matches_reference::<Blended>(&blended_sprite());
}

#[test]
fn masked_draws_fully_opaque_rows_like_opaque() {
    assert_matches_reference::<Masked>(&opaque_sprite());
}

#[test]
fn blended_draws_fully_opaque_rows_like_opaque() {
    assert_matches_reference::<Blended>(&opaque_sprite());
}

/// Renders a grid of letters: `a` is `color(1)`, `b` is `color(2)` and so on,
/// and `.` is the background.
fn grid(rows: &[&str]) -> Vec<Px> {
    rows.iter()
        .flat_map(|row| row.bytes())
        .map(|c| match c {
            b'.' => bg(),
            c => color(c - b'a' + 1),
        })
        .collect()
}

/// The source image for the literal grids:
/// ```text
/// abc
/// def
/// ```
fn letters() -> Sprite {
    Sprite::from_rows(&[
        &[(1, 255), (2, 255), (3, 255)],
        &[(4, 255), (5, 255), (6, 255)],
    ])
}

fn drawn<K: DrawAlpha<Px>>(sprite: &Sprite, size: (u16, u16), t: Transform) -> Vec<Px> {
    let buffer = vec![bg(); size.0 as usize * size.1 as usize];
    let mut canvas = Canvas::full(Vec2::new(size.0, size.1), buffer);
    canvas.draw_image(&sprite.image::<K>(), Vec2::new(0, 0), t);
    canvas.buffer
}

#[test]
fn each_transform_produces_the_documented_orientation() {
    let cases: [(Transform, (u16, u16), &[&str]); 8] = [
        (Transform::Identity, (3, 2), &["abc", "def"]),
        (Transform::Transpose, (2, 3), &["ad", "be", "cf"]),
        (Transform::FlipY, (3, 2), &["def", "abc"]),
        (Transform::RotCcw, (2, 3), &["cf", "be", "ad"]),
        (Transform::FlipX, (3, 2), &["cba", "fed"]),
        (Transform::RotCw, (2, 3), &["da", "eb", "fc"]),
        (Transform::Rot180, (3, 2), &["fed", "cba"]),
        (Transform::Transverse, (2, 3), &["fc", "eb", "da"]),
    ];
    let sprite = letters();
    for (t, size, rows) in cases {
        let expected = grid(rows);
        assert_eq!(drawn::<Opaque>(&sprite, size, t), expected, "opaque {t:?}");
        assert_eq!(drawn::<Masked>(&sprite, size, t), expected, "masked {t:?}");
        assert_eq!(
            drawn::<Blended>(&sprite, size, t),
            expected,
            "blended {t:?}"
        );
    }
}

#[test]
fn transparent_spans_leave_the_canvas_untouched() {
    let sprite = Sprite::from_rows(&[&[(1, 255), (0, 0), (3, 255)]]);
    assert_eq!(
        drawn::<Masked>(&sprite, (3, 1), Transform::Identity),
        grid(&["a.c"]),
    );
    assert_eq!(
        drawn::<Masked>(&sprite, (3, 1), Transform::FlipX),
        grid(&["c.a"]),
    );
}

#[test]
fn clipping_at_each_edge() {
    let sprite = letters();
    let cases: [(i16, i16, &[&str]); 4] = [
        (-1, 0, &["bc..", "ef..", "....", "...."]),
        (0, -1, &["def.", "....", "....", "...."]),
        (2, 0, &["..ab", "..de", "....", "...."]),
        (0, 3, &["....", "....", "....", "abc."]),
    ];
    for (x, y, rows) in cases {
        let mut canvas = Canvas::full(Vec2::new(4, 4), vec![bg(); 16]);
        canvas.draw_image(
            &sprite.image::<Opaque>(),
            Vec2::new(x, y),
            Transform::Identity,
        );
        assert_eq!(canvas.buffer, grid(rows), "({x}, {y})");
    }
}

#[test]
fn image_entirely_off_canvas_draws_nothing() {
    let sprite = letters();
    for (x, y) in [(-3, 0), (4, 0), (0, -2), (0, 4)] {
        let mut canvas = Canvas::full(Vec2::new(4, 4), vec![bg(); 16]);
        canvas.draw_image(
            &sprite.image::<Masked>(),
            Vec2::new(x, y),
            Transform::Identity,
        );
        assert_eq!(canvas.buffer, vec![bg(); 16], "({x}, {y})");
    }
}

#[test]
fn band_shows_only_its_slice_of_the_image() {
    let sprite = letters();
    let mut canvas = Canvas::new(Vec2::new(3, 2), (1..2).into(), vec![bg(); 3]);
    canvas.draw_image(
        &sprite.image::<Opaque>(),
        Vec2::new(0, 0),
        Transform::Identity,
    );
    assert_eq!(canvas.buffer, grid(&["def"]));
}

#[test]
fn blended_pixels_mix_with_the_canvas() {
    let sprite = Sprite::new(vec![vec![(Px::black(), 128)]]);
    let mut canvas = Canvas::full(Vec2::new(1, 1), vec![Px::white()]);
    canvas.draw_image(
        &sprite.image::<Blended>(),
        Vec2::new(0, 0),
        Transform::Identity,
    );
    // White is (31, 63, 31) in 565. Black at alpha 128 leaves 127/255 of it.
    assert_eq!(canvas.buffer, [Px::new(15, 31, 15)]);
}

#[test]
#[should_panic(expected = "Unexpected SpanKind::Blended")]
fn blended_span_in_masked_image_panics() {
    let sprite = Sprite::from_rows(&[&[(1, 128)]]);
    let mut canvas = Canvas::full(Vec2::new(1, 1), vec![bg()]);
    canvas.draw_image(
        &sprite.image::<Masked>(),
        Vec2::new(0, 0),
        Transform::Identity,
    );
}

#[test]
fn image_dimensions() {
    let sprite = opaque_sprite();
    assert_eq!(sprite.image::<Opaque>().size(), Vec2::new(4, 3));
    assert_eq!(sprite.image::<Masked>().size(), Vec2::new(4, 3));

    // Row count comes from row starts, even when rows have several spans.
    let sprite = masked_sprite();
    assert_eq!(sprite.image::<Masked>().size(), Vec2::new(4, 3));
}

#[test]
fn canvas_addresses_pixels_relative_to_its_band() {
    let mut canvas = Canvas::new(Vec2::new(2, 4), (2..4).into(), vec![bg(); 4]);
    canvas.write(Vec2::new(1, 3), color(1));
    assert_eq!(canvas.read(Vec2::new(1, 3)), color(1));
    assert_eq!(canvas.buffer[3], color(1));
}

#[test]
fn canvas_fill_covers_the_whole_buffer() {
    let mut canvas = Canvas::new(Vec2::new(2, 4), (2..4).into(), vec![bg(); 4]);
    canvas.fill(color(1));
    assert_eq!(canvas.buffer, vec![color(1); 4]);
}

#[test]
fn canvas_over_packed_binary_buffer() {
    let mut canvas = Canvas::full(Vec2::new(3, 3), BinaryColor::packed_vec(9));
    canvas.fill(BinaryColor::ON);
    canvas.write(Vec2::new(1, 1), BinaryColor::OFF);
    assert_eq!(canvas.read(Vec2::new(0, 0)), BinaryColor::ON);
    assert_eq!(canvas.read(Vec2::new(1, 1)), BinaryColor::OFF);
    assert_eq!(canvas.read(Vec2::new(2, 2)), BinaryColor::ON);
}

#[test]
fn compose_applies_self_then_other() {
    const SIZE: (i16, i16) = (3, 2);
    for a in TRANSFORMS {
        for b in TRANSFORMS {
            let composed = a.compose(b);
            for y in 0..SIZE.1 {
                for x in 0..SIZE.0 {
                    let step = place(a, (x, y), SIZE);
                    assert_eq!(
                        place(composed, (x, y), SIZE),
                        place(b, step, placed_size(a, SIZE)),
                        "{a:?} then {b:?}, source ({x}, {y})",
                    );
                }
            }
        }
    }
}

#[test]
fn rotations_compose_as_expected() {
    use Transform::*;
    assert_eq!(RotCw.compose(RotCw), Rot180);
    assert_eq!(RotCcw.compose(RotCcw), Rot180);
    assert_eq!(RotCw.compose(RotCcw), Identity);
    assert_eq!(Rot180.compose(RotCw), RotCcw);
    assert_eq!(FlipX.compose(FlipY), Rot180);
    assert_eq!(Transpose.compose(Transverse), Rot180);
}

#[test]
fn from_bits_matches_accessors() {
    for t in TRANSFORMS {
        let rebuilt = Transform::from_bits(t.is_flipped_x(), t.is_flipped_y(), t.is_transposed());
        assert_eq!(rebuilt, t);
    }
    assert_eq!(Transform::from_bits(true, false, true), Transform::RotCw);
    assert_eq!(Transform::from_bits(false, true, true), Transform::RotCcw);
}

#[test]
fn src_reversed_swaps_axes_when_transposed() {
    assert_eq!(Transform::FlipX.src_reversed(), Vec2::new(true, false));
    assert_eq!(Transform::RotCw.src_reversed(), Vec2::new(false, true));
    assert_eq!(Transform::RotCcw.src_reversed(), Vec2::new(true, false));
}

#[test]
fn bit_operators_act_on_each_flag() {
    use Transform::*;
    assert_eq!(FlipX | Transpose, RotCw);
    assert_eq!(RotCw & FlipX, FlipX);
    assert_eq!(Rot180 ^ FlipX, FlipY);
    assert_eq!(!Identity, Transverse);
    for t in TRANSFORMS {
        assert_eq!(!!t, t);
    }

    let mut t = FlipX;
    t |= Transpose;
    assert_eq!(t, RotCw);
    t &= Transpose;
    assert_eq!(t, Transpose);
    t ^= Transverse;
    assert_eq!(t, Rot180);
}

#[test]
#[should_panic(expected = "size greater than")]
fn rect_rejects_sizes_beyond_signed_range() {
    Rect::<W16>::new(Vec2::new(0, 0), Vec2::new(i16::MAX as u16 + 1, 0));
}

#[test]
fn rect_accepts_the_largest_signed_size() {
    let rect = Rect::<W16>::new(Vec2::new(-1, 2), Vec2::new(i16::MAX as u16, 3));
    assert_eq!(rect.right(), i16::MAX - 1);
    assert_eq!(rect.bottom(), 5);
}

/// Drives a future to completion, polling again whenever it is pending.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
}

/// Stays pending for one poll, so sinks behave like real asynchronous displays.
#[derive(Default)]
struct YieldOnce(bool);

impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            Poll::Pending
        }
    }
}

#[derive(Default)]
struct RecordingSink {
    frames: Vec<(Vec2<u16>, URect<W16>, Vec<Px>)>,
    fail_on: Option<usize>,
}

impl FrameSink for RecordingSink {
    type Px = Px;
    type Error = usize;

    async fn present<B: WritablePixelArray<Px>>(
        &mut self,
        size: Vec2<u16>,
        region: URect<W16>,
        canvas: &Canvas<Px, B>,
    ) -> Result<(), usize> {
        YieldOnce::default().await;
        let index = self.frames.len();
        if self.fail_on == Some(index) {
            return Err(index);
        }
        let pixels = (region.top()..region.bottom())
            .flat_map(|y| (region.left()..region.right()).map(move |x| (x, y)))
            .map(|(x, y)| canvas.read(Vec2::new(x, y)))
            .collect();
        self.frames.push((size, region, pixels));
        Ok(())
    }
}

fn renderer(size: (u16, u16), band_height: u16) -> Renderer<Px, Vec<Px>> {
    let len = size.0 as usize * band_height as usize;
    Renderer::new(
        Vec2::new(size.0, size.1),
        band_height,
        [vec![bg(); len], vec![bg(); len]],
    )
}

fn fill_by_band(canvas: &mut Canvas<Px, Vec<Px>>) {
    canvas.fill(color(canvas.band().start as u8 + 1));
}

#[test]
fn bands_cover_the_height_with_a_shorter_final_band() {
    let bands = |h, b| Renderer::<Px, Vec<Px>>::bands(h, b).collect::<Vec<_>>();
    assert_eq!(bands(5, 2), [(0..2).into(), (2..4).into(), (4..5).into()]);
    assert_eq!(bands(4, 2), [(0..2).into(), (2..4).into()]);
    assert_eq!(bands(2, 8), [(0..2).into()]);
    assert_eq!(bands(0, 2), []);
}

#[test]
fn render_presents_each_band_in_order_with_its_own_pixels() {
    let mut sink = RecordingSink::default();
    block_on(renderer((2, 5), 2).render(fill_by_band, &mut sink)).unwrap();

    let region = |y, h| URect::new(Vec2::new(0, y), Vec2::new(2, h));
    let size = Vec2::new(2, 5);
    assert_eq!(
        sink.frames,
        [
            (size, region(0, 2), vec![color(1); 4]),
            (size, region(2, 2), vec![color(3); 4]),
            (size, region(4, 1), vec![color(5); 2]),
        ]
    );
}

#[test]
fn render_stitches_an_image_across_bands() {
    let sprite = blended_sprite();
    let (pos, t) = (Vec2::new(1, -1), Transform::RotCw);
    let image = sprite.image::<Blended>();
    let mut sink = RecordingSink::default();
    let draw = |canvas: &mut Canvas<Px, Vec<Px>>| {
        canvas.fill(bg());
        canvas.draw_image(&image, pos, t);
    };
    block_on(renderer((5, 5), 2).render(draw, &mut sink)).unwrap();

    let frame: Vec<Px> = sink.frames.into_iter().flat_map(|f| f.2).collect();
    assert_eq!(frame, reference_frame(&sprite, (5, 5), pos, t));
}

#[test]
fn render_returns_the_error_from_a_middle_band() {
    let mut sink = RecordingSink {
        fail_on: Some(1),
        ..Default::default()
    };
    let result = block_on(renderer((2, 6), 2).render(fill_by_band, &mut sink));
    assert_eq!(result, Err(1));
    assert_eq!(sink.frames.len(), 1);
}

#[test]
fn render_returns_the_error_from_the_final_band() {
    let mut sink = RecordingSink {
        fail_on: Some(2),
        ..Default::default()
    };
    let result = block_on(renderer((2, 6), 2).render(fill_by_band, &mut sink));
    assert_eq!(result, Err(2));
    assert_eq!(sink.frames.len(), 2);
}

#[test]
fn resize_updates_canvas_geometry() {
    let mut renderer = renderer((2, 2), 1);
    renderer.resize(Vec2::new(4, 4), 2, [vec![bg(); 8], vec![bg(); 8]]);
    assert_eq!(renderer.size(), Vec2::new(4, 4));
    for canvas in &renderer.buffers {
        assert_eq!(canvas.size(), Vec2::new(4, 4));
        assert_eq!(canvas.band().end, 2);
    }
}

#[cfg(feature = "alloc")]
#[test]
fn allocating_constructors_size_buffers_for_one_band() {
    let mut renderer = Renderer::<Px, Vec<Px>>::with_size(Vec2::new(3, 4), 2);
    assert_eq!(renderer.size(), Vec2::new(3, 4));
    assert_eq!(renderer.buffers[0].buffer.len(), 6);

    renderer.resize_alloc(Vec2::new(5, 6), 3);
    assert_eq!(renderer.size(), Vec2::new(5, 6));
    for canvas in renderer.into_inner() {
        assert_eq!(canvas.buffer.len(), 15);
    }
}

struct Display {
    size: Size,
    pixels: Vec<Rgb565>,
}

impl Display {
    fn new(width: u32, height: u32) -> Self {
        Self {
            size: Size::new(width, height),
            pixels: vec![Px::black().into(); (width * height) as usize],
        }
    }
}

impl OriginDimensions for Display {
    fn size(&self) -> Size {
        self.size
    }
}

impl DrawTarget for Display {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I: IntoIterator<Item = EgPixel<Rgb565>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        for EgPixel(point, color) in pixels {
            self.pixels[point.y as usize * self.size.width as usize + point.x as usize] = color;
        }
        Ok(())
    }
}

#[test]
fn draw_target_sink_writes_every_band_to_the_display() {
    let mut sink = DrawTargetSink::<Px, _>::new(Display::new(2, 3));
    block_on(renderer((2, 3), 2).render(fill_by_band, &mut sink)).unwrap();

    let expected: Vec<Rgb565> = [1, 1, 1, 1, 3, 3]
        .map(|c| color(c).into())
        .into_iter()
        .collect();
    assert_eq!(sink.target.pixels, expected);
}

#[test]
#[should_panic(expected = "assertion `left == right` failed")]
fn draw_target_sink_rejects_a_mismatched_display_size() {
    let mut sink = DrawTargetSink::<Px, _>::new(Display::new(2, 2));
    let _ = block_on(renderer((2, 3), 2).render(fill_by_band, &mut sink));
}
