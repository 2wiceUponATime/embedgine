use embedgine::formats::Rgb888 as Px;

mod assets {
    use super::Px;
    use embedgine::include_assets;

    include_assets!();
}

fn main() {
    for img in [assets::OPAQUE, assets::MASKED, assets::ALPHA] {
        println!("{img:?}");
    }
}
