use std::{
    collections::BTreeMap,
    env::VarError,
    ffi::OsStr,
    fmt::Debug,
    fs, io,
    ops::{Deref, DerefMut},
    path::{Component, Path, PathBuf},
};

use convert_case::{Case, Casing};
use embedgine_formats::{Alpha, Pixel, Rgba8, Span, SpanKind};
use image::{GenericImageView, ImageError, ImageReader, Pixel as _};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use walkdir::WalkDir;

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
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug)]
enum EntryKind {
    Image {
        px_path: PathBuf,
        alpha_path: Option<PathBuf>,
        alpha: Alpha,
        width: u16,
        pixels_len: usize,
        spans: Vec<Span>,
        row_starts: Vec<u16>,
    },
    Module(Module),
}

#[derive(Debug)]
struct Entry {
    path: PathBuf,
    kind: EntryKind,
}

impl Entry {
    pub fn new(path: PathBuf, kind: EntryKind) -> Self {
        Self { path, kind }
    }
}

#[derive(Debug, Default)]
struct Module {
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

    fn generate_inner(&self) -> TokenStream {
        self.entries.iter().map(|(name, entry)| {
            let name = format_ident!("{name}");
            match &entry.kind {
                EntryKind::Image { px_path, alpha_path, alpha, width, row_starts, pixels_len, spans } => {
                    let row_starts: &[u16] = if *alpha == Alpha::Opaque { &[] } else { row_starts };
                    let px_path = px_path.to_str().unwrap();
                    let spans = spans
                        .iter()
                        .map(|&Span { len, kind }| {
                            let kind = format_ident!("{kind:?}");
                            quote! {
                                embedgine::formats::Span {
                                    len: #len,
                                    kind: embedgine::formats::SpanKind::#kind,
                                }
                            }
                        });
                    let alpha_path = match alpha_path {
                        Some(path) => {
                            let path = path.to_str().unwrap();
                            quote! { include_bytes!(#path) }
                        }
                        None => quote! { &[] }
                    };
                    let alpha = format_ident!("{alpha:?}");
                    quote! {
                        pub const #name: embedgine::Image<'static, Px, embedgine::formats::#alpha> = {
                            const DATA: &Aligned<Px, [u8; #pixels_len * std::mem::size_of::<Px>()]> = &Aligned {
                                _align: [],
                                bytes: *include_bytes!(#px_path)
                            };
                            embedgine::Image::new(
                                #width,
                                unsafe { core::slice::from_raw_parts(DATA.bytes.as_ptr().cast(), #pixels_len) },
                                #alpha_path,
                                &[#(#spans),*],
                                &[#(#row_starts),*],
                            )
                        };
                    }
                }
                EntryKind::Module(module) => {
                    let body = module.generate_inner();
                    quote! {
                        pub mod #name {
                            use super::*;
                            #body
                        }
                    }
                }
            }
        }).collect()
    }

    pub fn generate<Px: Pixel>(&self) -> String {
        let body = self.generate_inner();
        let format = Px::FORMAT;
        let file = quote! {
            #[repr(C)]
            struct Aligned<A, B: ?Sized> {
                _align: [A; 0],
                bytes: B,
            }
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

fn utf8(os: &OsStr, path: impl AsRef<Path>) -> Result<String, Error> {
    Ok(os
        .to_str()
        .ok_or_else(|| Error::InvalidPath(path.as_ref().to_path_buf()))?
        .to_owned())
}

pub fn build<Px: Pixel + Debug>(assets_path: impl AsRef<Path>) -> Result<(), Error> {
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
        let image = ImageReader::open(entry.path())?
            .decode()
            .map_err(|e| Error::Image(entry.path().to_path_buf(), e))?;
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
                    row_starts.push(spans.len() as u16);
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
        let relative_path = entry.path().strip_prefix(&assets_path).unwrap();
        let base_out_path = out_dir.join(relative_path);
        let px_path = base_out_path.with_extension("px.bin");
        let mut dirs = relative_path
            .components()
            .filter_map(|c| match c {
                Component::Normal(os) => Some(utf8(os, relative_path)),
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
        let stem = utf8(entry.path().file_stem().unwrap(), entry.path())?;
        if let Some(parent) = px_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&px_path, bytemuck::cast_slice(&pixels))?;
        let alpha_path = if !alphas.is_empty() {
            let path = base_out_path.with_extension("alpha.bin");
            fs::write(&path, &alphas)?;
            Some(path)
        } else {
            None
        };
        stack.last_mut().unwrap().module.insert(
            stem.to_case(Case::UpperSnake),
            Entry::new(
                entry.path().to_path_buf(),
                EntryKind::Image {
                    px_path,
                    alpha_path,
                    alpha: alpha_kind,
                    width: width as u16,
                    pixels_len: pixels.len(),
                    spans,
                    row_starts,
                },
            ),
        )?;
        println!("{entry:?} - {width}x{height}");
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
