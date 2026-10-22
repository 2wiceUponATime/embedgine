use embedded_graphics::{geometry::Size, pixelcolor};
use embedded_graphics_simulator::{SimulatorDisplay, Window};
use embedgine::{Canvas, DrawTargetSink, Renderer, formats::Rgb565Le as Px, geometry::Vec2};

mod assets {
    use super::Px;
    use embedgine::include_assets;

    include_assets!();
}

const WIDTH: u16 = 640;
const HEIGHT: u16 = 360;

#[tokio::main]
async fn main() {
    let display =
        SimulatorDisplay::<pixelcolor::Rgb565>::new(Size::new(WIDTH as u32, HEIGHT as u32));
    let mut sink = DrawTargetSink::<Px, _>::new(display);

    let mut renderer: Renderer<Px, _> = Renderer::with_size(Vec2::new(WIDTH, HEIGHT), 18);

    renderer
        .render(
            |c: &mut Canvas<Px, _>| {
                let v = (c.band().start as u32 * 256 / HEIGHT as u32) as u8;
                c.fill(Px::from_rgb8(v, v, v));
            },
            &mut sink,
        )
        .await
        .unwrap();

    Window::new("Embedgine Demo", &Default::default()).show_static(&sink.target);
}
