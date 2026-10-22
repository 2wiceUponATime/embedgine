use embedgine_build::build;
use embedgine_formats::Rgb565Le as Px;

fn main() {
    if let Err(e) = build::<Px>("assets") {
        panic!("{e}");
    }
}
