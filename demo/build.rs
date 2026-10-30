use embedgine_build::build;
use embedgine_formats::embedded_graphics::Rgb565Le as Px;

fn main() {
    if let Err(e) = build::<Px>("assets") {
        panic!("{e}");
    }
}
