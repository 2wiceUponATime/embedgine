use embedgine_build::build;
use embedgine_formats::Rgb888;

fn main() {
    if let Err(e) = build::<Rgb888>("assets") {
        panic!("{e}");
    }
}
