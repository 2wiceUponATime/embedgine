use std::{
    collections::BTreeMap,
    env::VarError,
    error::Error as ErrorTrait,
    ffi::OsStr,
    fmt::Debug,
    fs, io,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    path::{Component, Path, PathBuf},
};

use convert_case::{Case, Casing};
use embedgine_formats::{Alpha, PackedPixelArray, Pixel, Rgba8, RowStart, Span, SpanKind};
use image::{DynamicImage, GenericImageView, ImageError, ImageReader, Pixel as _};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("error reading OUT_DIR: {0}")]
    InvalidOutDir(VarError),
    #[error("assets root {0} must be a directory")]
    InvalidRoot(PathBuf),
    #[error("error decoding path {0}")]
    InvalidPath(PathBuf),
    #[error("{0}")]
    Io(io::Error),
    #[error("error opening image {0}: {1}")]
    Image(PathBuf, ImageError),
    #[error("{0} collided with previous instance of '{1}'")]
    Collision(PathBuf, String),
    #[error("{0}")]
    Other(Box<dyn ErrorTrait>),
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug)]
pub enum EntryKind {
    Image {
        px_path: PathBuf,
        alpha_path: Option<PathBuf>,
        alpha: Alpha,
        width: u16,
        spans: Vec<Span>,
        row_starts: Vec<RowStart>,
    },
    Module(Module),
    Tokens(TokenStream),
}

impl EntryKind {
    fn to_tokens(&self, name: Ident) -> TokenStream {
        match self {
            Self::Image {
                px_path,
                alpha_path,
                alpha,
                width,
                spans,
                row_starts,
            } => {
                let row_starts: Vec<TokenStream> = if *alpha == Alpha::Opaque {
                    vec![]
                } else {
                    row_starts
                        .iter()
                        .map(
                            |&RowStart {
                                 px_idx,
                                 alpha_idx,
                                 span_idx,
                             }| {
                                quote! {
                                    embedgine::formats::RowStart {
                                        px_idx: #px_idx,
                                        alpha_idx: #alpha_idx,
                                        span_idx: #span_idx,
                                    }
                                }
                            },
                        )
                        .collect()
                };
                let px_path = px_path.to_str().unwrap();
                let spans = spans.iter().map(|&Span { len, kind }| {
                    let kind = format_ident!("{kind:?}");
                    quote! {
                        embedgine::formats::Span {
                            len: #len,
                            kind: embedgine::formats::SpanKind::#kind,
                        }
                    }
                });
                let alphas = match alpha_path {
                    Some(path) => {
                        let path = path.to_str().unwrap();
                        quote! { include_bytes!(#path) }
                    }
                    None => quote! { &[] },
                };
                let alpha = format_ident!("{alpha:?}");
                quote! {
                    pub const #name: embedgine::Image<'static, Px, embedgine::formats::#alpha> = {
                        embedgine::Image::new(
                            #width,
                            unsafe { embedgine::formats::cast_bytes(include_bytes!(#px_path)) },
                            #alphas,
                            &[#(#spans),*],
                            &[#(#row_starts),*],
                        )
                    };
                }
            }
            Self::Module(module) => {
                let body = module.to_tokens();
                quote! {
                    pub mod #name {
                        #body
                    }
                }
            }
            Self::Tokens(stream) => stream.clone(),
        }
    }
}

#[derive(Debug)]
pub struct Entry {
    path: PathBuf,
    kind: EntryKind,
}

impl Entry {
    pub fn new(path: PathBuf, kind: EntryKind) -> Self {
        Self { path, kind }
    }

    pub fn to_tokens(&self, name: Ident) -> TokenStream {
        self.kind.to_tokens(name)
    }
}

#[derive(Debug, Default)]
pub struct Module {
    entries: BTreeMap<String, Entry>,
}

impl Module {
    pub fn insert(&mut self, key: String, value: Entry) -> Result<(), Error> {
        if self.entries.contains_key(&key) {
            return Err(Error::Collision(value.path, key));
        }
        self.entries.insert(key, value);
        Ok(())
    }

    fn to_tokens(&self) -> TokenStream {
        self.entries
            .iter()
            .map(|(name, entry)| entry.to_tokens(format_ident!("{name}")))
            .collect()
    }

    pub fn generate<Px: Pixel>(&self) -> String {
        let body = self.to_tokens();
        let format = Px::FORMAT;
        let file = quote! {
            const _: () = {
                const fn bytes_equal(lhs: &[u8], rhs: &[u8]) -> bool {
                    if lhs.len() != rhs.len() {
                        return false;
                    }
                    let mut i = 0;
                    while i < lhs.len() {
                        if lhs[i] != rhs[i] {
                            return false;
                        }
                        i += 1;
                    }
                    true
                }
                assert!(
                    bytes_equal(
                        <Px as embedgine::formats::Pixel>::FORMAT.as_bytes(),
                        #format.as_bytes(),
                    ), "pixel format mismatch"
                );
            };
            #body
        };
        prettyplease::unparse(&syn::parse2(file).unwrap())
    }
}

#[derive(Debug)]
struct Frame {
    name: String,
    module: Module,
    path: PathBuf,
}

struct Stack(Vec<Frame>);

impl Stack {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self(vec![Frame {
            name: String::new(),
            module: Module::default(),
            path: path.as_ref().to_path_buf(),
        }])
    }

    pub fn pop_frame(&mut self) -> Result<(), Error> {
        let frame = self.pop().unwrap();
        self.last_mut()
            .unwrap()
            .module
            .insert(
                frame.name.to_case(Case::Snake),
                Entry::new(frame.path, EntryKind::Module(frame.module)),
            )
            .map(|_| ())
    }
}

impl Deref for Stack {
    type Target = Vec<Frame>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Stack {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub fn utf8_or_else<E>(os: &OsStr, f: impl FnOnce() -> E) -> Result<String, E> {
    Ok(os.to_str().ok_or_else(f)?.to_owned())
}

pub fn utf8_from_path(os: &OsStr, path: impl AsRef<Path>) -> Result<String, Error> {
    utf8_or_else(os, || Error::InvalidPath(path.as_ref().to_path_buf()))
}

pub trait Matcher {
    fn matches(&self, path: &Path) -> bool;
}

pub struct Ext<'a>(pub &'a str);

impl Matcher for Ext<'_> {
    fn matches(&self, path: &Path) -> bool {
        path.extension()
            .is_some_and(|os| os.to_str().is_some_and(|ext| ext == self.0))
    }
}

pub struct AnyOf<T, M>(pub T, PhantomData<M>);

impl<T, M> AnyOf<T, M> {
    pub fn new(value: T) -> Self {
        Self(value, PhantomData)
    }
}

impl<T: AsRef<[M]>, M: Matcher> Matcher for AnyOf<T, M> {
    fn matches(&self, path: &Path) -> bool {
        for m in self.0.as_ref() {
            if m.matches(path) {
                return true;
            }
        }
        false
    }
}

pub struct AllOf<T, M>(pub T, PhantomData<M>);

impl<T, M> AllOf<T, M> {
    pub fn new(value: T) -> Self {
        Self(value, PhantomData)
    }
}

impl<T: AsRef<[M]>, M: Matcher> Matcher for AllOf<T, M> {
    fn matches(&self, path: &Path) -> bool {
        for m in self.0.as_ref() {
            if !m.matches(path) {
                return false;
            }
        }
        true
    }
}

impl<T: Fn(&Path) -> bool> Matcher for T {
    fn matches(&self, path: &Path) -> bool {
        self(path)
    }
}

impl Matcher for Box<dyn Matcher + '_> {
    fn matches(&self, path: &Path) -> bool {
        (**self).matches(path)
    }
}

pub trait Handler<Px> {
    fn handle(&self, path: &Path) -> Result<Vec<(String, EntryKind)>, Error>;
}

impl<T: Fn(&Path) -> Result<Vec<(String, EntryKind)>, Error>, Px> Handler<Px> for T {
    fn handle(&self, path: &Path) -> Result<Vec<(String, EntryKind)>, Error> {
        self(path)
    }
}

pub struct Skip;

impl<Px> Handler<Px> for Skip {
    fn handle(&self, _: &Path) -> Result<Vec<(String, EntryKind)>, Error> {
        Ok(vec![])
    }
}

pub type HandlerPair<'a, Px> = (Box<dyn Matcher + 'a>, Box<dyn Handler<Px> + 'a>);

#[derive(Default)]
pub struct Builder<'a, Px> {
    handlers: Vec<HandlerPair<'a, Px>>,
}

impl<'a, Px> Builder<'a, Px> {
    pub fn new() -> Self
    where
        Px: Default,
    {
        Self::default()
    }

    pub fn handler(mut self, matcher: impl Matcher + 'a, handler: impl Handler<Px> + 'a) -> Self {
        self.handlers.push((Box::new(matcher), Box::new(handler)));
        self
    }

    pub fn build(self, assets_path: impl AsRef<Path>) -> Result<(), Error>
    where
        Px: Pixel,
    {
        println!("cargo::rerun-if-changed={}", assets_path.as_ref().display());

        let out_dir = PathBuf::from(std::env::var("OUT_DIR").map_err(Error::InvalidOutDir)?)
            .join("embedgine-build");
        if !fs::metadata(&assets_path)?.is_dir() {
            return Err(Error::InvalidRoot(assets_path.as_ref().to_path_buf()));
        }
        fs::create_dir_all(&out_dir)?;
        let mut stack = Stack::new(&assets_path);
        for entry in WalkDir::new(&assets_path)
            .follow_links(true)
            .sort_by_file_name()
        {
            let entry = entry.map_err(io::Error::from)?;
            if !entry.file_type().is_file() {
                continue;
            }
            let relative_path = entry.path().strip_prefix(&assets_path).unwrap();
            let mut dirs = relative_path
                .components()
                .filter_map(|c| match c {
                    Component::Normal(os) => Some(utf8_from_path(os, relative_path)),
                    _ => None,
                })
                .collect::<Result<Vec<_>, _>>()?;
            dirs.pop();
            let common = dirs
                .iter()
                .zip(stack.iter().skip(1))
                .take_while(|(name, frame)| **name == frame.name)
                .count();
            while stack.len() > common + 1 {
                stack.pop_frame()?;
            }
            for name in dirs.into_iter().skip(common) {
                let path = stack.last().unwrap().path.join(&name);
                stack.push(Frame {
                    name,
                    module: Module::default(),
                    path,
                });
            }
            let last = stack.last_mut().unwrap();
            for (name, kind) in self.build_entry(assets_path.as_ref(), &out_dir, &entry)? {
                last.module
                    .insert(name, Entry::new(entry.path().to_path_buf(), kind))?;
            }
        }
        while stack.len() > 1 {
            stack.pop_frame()?;
        }
        fs::write(
            out_dir.join("assets.rs"),
            stack.0[0].module.generate::<Px>(),
        )?;
        Ok(())
    }

    fn build_entry(
        &self,
        assets_path: &Path,
        out_dir: &Path,
        entry: &DirEntry,
    ) -> Result<Vec<(String, EntryKind)>, Error>
    where
        Px: Pixel,
    {
        for (m, h) in &self.handlers {
            if m.matches(entry.path()) {
                return h.handle(entry.path());
            }
        }
        let image = ImageReader::open(entry.path())?
            .decode()
            .map_err(|e| Error::Image(entry.path().to_path_buf(), e))?;
        let EncodedImage {
            pixels,
            alphas,
            alpha: alpha_kind,
            width,
            spans,
            row_starts,
        } = encode_image::<Px>(&image);
        let relative_path = entry.path().strip_prefix(assets_path).unwrap();
        let base_out_path = out_dir.join(relative_path);
        let px_path = base_out_path.with_added_extension("px.bin");
        if let Some(parent) = px_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let packed = Px::Array::pack(&pixels);
        fs::write(&px_path, &packed)?;
        let alpha_path = if !alphas.is_empty() {
            let path = base_out_path.with_extension("alpha.bin");
            fs::write(&path, &alphas)?;
            Some(path)
        } else {
            None
        };
        let stem = utf8_from_path(entry.path().file_stem().unwrap(), entry.path())?;
        Ok(vec![(
            stem.to_case(Case::Constant),
            EntryKind::Image {
                px_path,
                alpha_path,
                alpha: alpha_kind,
                width,
                spans,
                row_starts,
            },
        )])
    }
}

struct EncodedImage<Px> {
    pixels: Vec<Px>,
    alphas: Vec<u8>,
    alpha: Alpha,
    width: u16,
    spans: Vec<Span>,
    row_starts: Vec<RowStart>,
}

fn encode_image<Px: Pixel>(image: &DynamicImage) -> EncodedImage<Px> {
    let (width, height) = image.dimensions();
    let pixel_count = width as usize * height as usize;
    let mut pixels = Vec::with_capacity(pixel_count);
    let mut alphas = Vec::with_capacity(pixel_count);
    let mut alpha_kind = Alpha::Opaque;
    let mut spans: Vec<Span> = Vec::new();
    let mut row_starts = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let color = Rgba8::from_values(image.get_pixel(x, y).to_rgba().0);
            let kind = SpanKind::from_alpha(color.a);
            let alpha: Alpha = kind.into();
            alpha_kind = alpha_kind.max(alpha);
            if x == 0 {
                row_starts.push(RowStart {
                    px_idx: pixels.len() as u32,
                    alpha_idx: alphas.len() as u32,
                    span_idx: spans.len() as u16,
                });
            }
            if x == 0 || spans.last().is_none_or(|s| Alpha::from(s.kind) != alpha) {
                spans.push(Span { len: 0, kind });
            }
            let span = spans.last_mut().unwrap();
            span.len += 1;
            if kind == SpanKind::Blended {
                alphas.push(color.a);
            }
            if kind != SpanKind::Transparent {
                pixels.push(Px::from(color.into()));
            }
        }
    }
    EncodedImage {
        pixels,
        alphas,
        alpha: alpha_kind,
        width: width as u16,
        spans,
        row_starts,
    }
}

pub fn build<Px: Pixel>(assets_path: impl AsRef<Path>) -> Result<(), Error> {
    Builder::<Px>::new().build(assets_path)
}

#[cfg(test)]
mod tests {
    use embedgine_formats::Rgb888;
    use image::{Rgba, RgbaImage};

    use super::*;

    const T: [u8; 4] = [0, 0, 0, 0];

    fn solid(v: u8) -> [u8; 4] {
        [v, v, v, 255]
    }

    fn translucent(v: u8, a: u8) -> [u8; 4] {
        [v, v, v, a]
    }

    fn encode(rows: &[&[[u8; 4]]]) -> EncodedImage<Rgb888> {
        let image = RgbaImage::from_fn(rows[0].len() as u32, rows.len() as u32, |x, y| {
            Rgba(rows[y as usize][x as usize])
        });
        encode_image(&DynamicImage::ImageRgba8(image))
    }

    fn spans(e: &EncodedImage<Rgb888>) -> Vec<(u16, SpanKind)> {
        e.spans.iter().map(|s| (s.len, s.kind)).collect()
    }

    fn rows(e: &EncodedImage<Rgb888>) -> Vec<(u32, u32, u16)> {
        e.row_starts
            .iter()
            .map(|r| (r.px_idx, r.alpha_idx, r.span_idx))
            .collect()
    }

    fn reds(e: &EncodedImage<Rgb888>) -> Vec<u8> {
        e.pixels.iter().map(|p| p.r).collect()
    }

    #[test]
    fn opaque_image_has_one_span_per_row() {
        let e = encode(&[&[solid(1), solid(2)], &[solid(3), solid(4)]]);
        assert_eq!(e.alpha, Alpha::Opaque);
        assert_eq!(e.width, 2);
        assert_eq!(reds(&e), [1, 2, 3, 4]);
        assert!(e.alphas.is_empty());
        assert_eq!(spans(&e), [(2, SpanKind::Opaque), (2, SpanKind::Opaque)]);
        assert_eq!(rows(&e), [(0, 0, 0), (2, 0, 1)]);
    }

    #[test]
    fn transparent_pixels_store_no_data() {
        let e = encode(&[&[T, solid(1), solid(2), T], &[solid(3), T, T, solid(4)]]);
        assert_eq!(e.alpha, Alpha::Masked);
        assert_eq!(reds(&e), [1, 2, 3, 4]);
        assert!(e.alphas.is_empty());
        assert_eq!(
            spans(&e),
            [
                (1, SpanKind::Transparent),
                (2, SpanKind::Opaque),
                (1, SpanKind::Transparent),
                (1, SpanKind::Opaque),
                (2, SpanKind::Transparent),
                (1, SpanKind::Opaque),
            ]
        );
        assert_eq!(rows(&e), [(0, 0, 0), (2, 0, 3)]);
    }

    #[test]
    fn alphas_are_stored_only_for_blended_spans() {
        let e = encode(&[
            &[translucent(1, 10), translucent(2, 20), solid(3)],
            &[T, solid(4), translucent(5, 30)],
        ]);
        assert_eq!(e.alpha, Alpha::Blended);
        assert_eq!(reds(&e), [1, 2, 3, 4, 5]);
        assert_eq!(e.alphas, [10, 20, 30]);
        assert_eq!(
            spans(&e),
            [
                (2, SpanKind::Blended),
                (1, SpanKind::Opaque),
                (1, SpanKind::Transparent),
                (1, SpanKind::Opaque),
                (1, SpanKind::Blended),
            ]
        );
        assert_eq!(rows(&e), [(0, 0, 0), (3, 2, 2)]);
    }

    #[test]
    fn identical_kinds_do_not_merge_across_rows() {
        let e = encode(&[&[T, T], &[T, T]]);
        assert_eq!(e.alpha, Alpha::Masked);
        assert!(e.pixels.is_empty());
        assert_eq!(
            spans(&e),
            [(2, SpanKind::Transparent), (2, SpanKind::Transparent)]
        );
    }

    #[test]
    fn alpha_kind_does_not_depend_on_pixel_order() {
        assert_eq!(encode(&[&[translucent(1, 9), T]]).alpha, Alpha::Blended);
        assert_eq!(encode(&[&[T, translucent(1, 9)]]).alpha, Alpha::Blended);
    }

    fn image_entry() -> EntryKind {
        EntryKind::Image {
            px_path: "px.bin".into(),
            alpha_path: Some("alpha.bin".into()),
            alpha: Alpha::Blended,
            width: 1,
            spans: vec![Span {
                len: 1,
                kind: SpanKind::Blended,
            }],
            row_starts: vec![RowStart {
                px_idx: 0,
                alpha_idx: 0,
                span_idx: 0,
            }],
        }
    }

    #[test]
    fn generated_code_is_no_std_compatible() {
        let mut nested = Module::default();
        nested
            .insert(
                "INNER".into(),
                Entry::new("inner.png".into(), image_entry()),
            )
            .unwrap();
        let mut root = Module::default();
        root.insert(
            "OUTER".into(),
            Entry::new("outer.png".into(), image_entry()),
        )
        .unwrap();
        root.insert(
            "nested".into(),
            Entry::new("nested".into(), EntryKind::Module(nested)),
        )
        .unwrap();

        let generated = root.generate::<Rgb888>();

        assert!(generated.contains("pub const OUTER"), "{generated}");
        assert!(generated.contains("pub mod nested"), "{generated}");
        assert!(generated.contains("pub const INNER"), "{generated}");
        assert!(!generated.contains("std::"), "{generated}");
    }

    #[test]
    fn duplicate_module_entries_are_rejected() {
        let mut module = Module::default();
        module
            .insert("A".into(), Entry::new("a.png".into(), image_entry()))
            .unwrap();
        let err = module
            .insert("A".into(), Entry::new("a.bmp".into(), image_entry()))
            .unwrap_err();
        assert!(
            matches!(err, Error::Collision(path, key) if path == Path::new("a.bmp") && key == "A")
        );
    }

    #[test]
    fn matchers() {
        let png = Path::new("a/b.png");
        let bmp = Path::new("a/b.bmp");
        assert!(Ext("png").matches(png));
        assert!(!Ext("png").matches(bmp));
        assert!(!Ext("png").matches(Path::new("png")));

        let any = AnyOf::new([Ext("png"), Ext("bmp")]);
        assert!(any.matches(png) && any.matches(bmp));
        assert!(!any.matches(Path::new("a/b.gif")));

        let all = AllOf::new([Ext("png"), Ext("bmp")]);
        assert!(!all.matches(png));
        assert!(AllOf::new([Ext("png")]).matches(png));
    }
}
