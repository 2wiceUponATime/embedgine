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
use embedgine_formats::{Pixel, Rgba8};
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
        path: PathBuf,
        width: u32,
        height: u32,
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
                EntryKind::Image { path, width, height } => {
                    let len = *width as usize * *height as usize;
                    let path = path.to_str().unwrap();
                    quote! {
                        pub const #name: embedgine::Image<Px, &'static [Px]> = {
                            const DATA: &Aligned<Px, [u8; #len * std::mem::size_of::<Px>()]> = &Aligned {
                                _align: [],
                                bytes: *include_bytes!(#path)
                            };
                            embedgine::Image::new(
                                #width,
                                #height,
                                unsafe { core::slice::from_raw_parts(DATA.bytes.as_ptr().cast(), #len) }
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
        let mut pixels =
            Vec::with_capacity(width as usize * height as usize * std::mem::size_of::<Px>());
        for y in 0..height {
            for x in 0..width {
                let pixel = Px::from(Rgba8::from_values(image.get_pixel(x, y).to_rgba().0));
                pixels.push(pixel);
            }
        }
        let relative_path = entry.path().strip_prefix(&assets_path).unwrap();
        let out_path = out_dir.join(relative_path).with_extension("bin");
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
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out_path, bytemuck::cast_slice(&pixels))?;
        stack.last_mut().unwrap().module.insert(
            stem.to_case(Case::UpperSnake),
            Entry::new(
                entry.path().to_path_buf(),
                EntryKind::Image {
                    path: out_path,
                    width,
                    height,
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
