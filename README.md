# Embedgine

Embedgine is a no_std game engine for embedded targets.

```rust
#![no_std]

use embedgine::{
    Canvas, Renderer, Transform, formats::Rgb888 as Px, geometry::Vec2,
};

mod assets {
    use super::Px;
    use embedgine::include_assets;

    // Requires having generated the assets in build.rs using embedgine-build
    include_assets!();
}

async fn main() {
    let buffers = [[Px::black(); WIDTH as usize * HEIGHT as usize]; 2];

    // If band_height < height, rendering will be done in multiple strips
    let mut renderer: Renderer<Px, _> = Renderer::new(Vec2::new(WIDTH, HEIGHT), HEIGHT, buffers);

    renderer
        .render(
            |c: &mut Canvas<Px, _>| {
                c.draw_image(&assets::IMAGE, Vec2::new(0, 0), Transform::Identity);
            },
            // impl FrameSink<Px = Px>
            &mut sink,
        )
        .await
        .unwrap();
}
```

## AI Disclosure

AI was involved in writing tests and some other parts of the code.
