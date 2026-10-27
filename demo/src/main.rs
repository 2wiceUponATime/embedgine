use embedded_graphics::{geometry::Size, pixelcolor};
use embedded_graphics_simulator::{SimulatorDisplay, Window};
use embedgine::{
    Canvas, DrawTargetSink, Renderer, Transform, formats::Rgb565Le as Px, geometry::Vec2,
};

mod assets {
    use super::Px;
    use embedgine::include_assets;

    include_assets!();
}

const WIDTH: u16 = 160;
const HEIGHT: u16 = 128;

#[tokio::main]
async fn main() {
    let display =
        SimulatorDisplay::<pixelcolor::Rgb565>::new(Size::new(WIDTH as u32, HEIGHT as u32));
    let mut sink = DrawTargetSink::<Px, _>::new(display);

    let mut renderer: Renderer<Px, _> = Renderer::with_size(Vec2::new(WIDTH, HEIGHT), 4);

    renderer
        .render(
            |c: &mut Canvas<Px, _>| {
                let v = (c.band().start as u32 * 256 / HEIGHT as u32) as u8;
                c.fill(Px::from_rgb8(v, v, v));
                let img: embedgine::Image<'_, Px, embedgine::formats::Opaque> = assets::OPAQUE;
                let mut offset = img.width();
                for t in [
                    Transform::Identity,
                    Transform::FlipX,
                    Transform::FlipY,
                    Transform::Rot180,
                ] {
                    for far_edge in [false, true] {
                        c.draw_image(
                            &img,
                            Vec2::new(
                                if far_edge {
                                    (WIDTH - img.width() + 1) as i16
                                } else {
                                    -1
                                },
                                offset as i16,
                            ),
                            t,
                        );
                        c.draw_image(
                            &img,
                            Vec2::new(
                                offset as i16,
                                if far_edge {
                                    (HEIGHT - img.height() + 1) as i16
                                } else {
                                    -1
                                },
                            ),
                            t,
                        );
                        c.draw_image(
                            &img,
                            Vec2::new(
                                if far_edge {
                                    (WIDTH - img.height() + 1) as i16
                                } else {
                                    -1
                                },
                                (offset + img.height() + 1) as i16,
                            ),
                            t | Transform::Transpose,
                        );
                        c.draw_image(
                            &img,
                            Vec2::new(
                                (offset + img.width() + 1) as i16,
                                if far_edge {
                                    (HEIGHT - img.width() + 1) as i16
                                } else {
                                    -1
                                },
                            ),
                            t | Transform::Transpose,
                        );
                    }
                    offset += img.width() + img.height() + 2;
                }
            },
            &mut sink,
        )
        .await
        .unwrap();

    Window::new("Embedgine Demo", &Default::default()).show_static(&sink.target);
}
