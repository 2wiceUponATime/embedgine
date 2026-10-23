use embedded_graphics::{geometry::Size, pixelcolor};
use embedded_graphics_simulator::{SimulatorDisplay, Window};
use embedgine::{Canvas, DrawTargetSink, Renderer, formats::Rgb565Le as Px, geometry::Vec2};

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
                let img = assets::OPAQUE;
                for x in [
                    -1,
                    ((WIDTH - img.width()) / 2) as i16,
                    (WIDTH - img.width() + 1) as i16,
                ] {
                    for y in [
                        -1,
                        ((HEIGHT - img.height()) / 2) as i16,
                        (HEIGHT - img.height() + 1) as i16,
                    ] {
                        c.draw_image(&img, Vec2::new(x, y));
                    }
                }
            },
            &mut sink,
        )
        .await
        .unwrap();

    Window::new("Embedgine Demo", &Default::default()).show_static(&sink.target);
}
