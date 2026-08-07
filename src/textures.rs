//! Texture catalog + thumbnail infrastructure.
//!
//! Scans `assets/TEXTURES` for paintable textures, provides a single Repeat-sampler
//! loader for tiling, and manages a bounded LRU cache of egui thumbnails so the
//! texture browser can lazily preview thousands of textures without loading them
//! all into GPU memory at once.

use bevy::asset::AssetServer;
use bevy::image::{Image, ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy_egui::egui;
use std::collections::{HashMap, HashSet, VecDeque};

/// Folder (relative to the crate root) containing the texture pack.
pub const TEXTURES_DIR: &str = "assets/TEXTURES";

/// Upper bound on the number of thumbnails uploaded to egui at once.
const THUMB_CAP: usize = 256;

//------------------------------CATALOG-------------------------------

/// Directory scan result: category name -> sorted texture paths (assets-relative).
#[derive(Resource, Default)]
pub struct TextureCatalog {
    pub categories: Vec<String>,
    pub paths: Vec<Vec<String>>,
}

impl TextureCatalog {
    /// Paths for one category, or `None` if the category doesn't exist.
    pub fn category_paths(&self, category: &str) -> Option<&Vec<String>> {
        self.categories
            .iter()
            .position(|c| c == category)
            .map(|i| &self.paths[i])
    }
}

/// One-time scan of `assets/TEXTURES`: every subdirectory becomes a category.
fn scan_texture_catalog() -> TextureCatalog {
    const EXTS: [&str; 6] = ["jpg", "jpeg", "png", "webp", "gif", "bmp"];
    let mut categories = Vec::new();
    let mut paths = Vec::new();

    let Ok(entries) = std::fs::read_dir(TEXTURES_DIR) else {
        return TextureCatalog::default();
    };
    let mut dirs: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    dirs.sort();

    for dir in dirs {
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(format!("{TEXTURES_DIR}/{dir}")) {
            for e in entries.filter_map(|e| e.ok()) {
                let name = e.file_name().to_string_lossy().into_owned();
                let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
                if EXTS.contains(&ext.as_str()) {
                    files.push(format!("TEXTURES/{dir}/{name}"));
                }
            }
        }
        files.sort();
        if !files.is_empty() {
            categories.push(dir);
            paths.push(files);
        }
    }

    TextureCatalog { categories, paths }
}

//------------------------------LOADING-------------------------------

/// Load a texture with a Repeat sampler so it tiles in the 3D preview. This is
/// the single entry point for every texture the editor assigns, so the sampler
/// stays consistent (including after a save/load round trip).
pub fn load_repeat(server: &AssetServer, path: &str) -> Handle<Image> {
    let repeat = |s: &mut ImageLoaderSettings| {
        s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..default()
        });
    };
    server.load_builder().with_settings(repeat).load(path.to_string())
}

//------------------------------THUMBNAILS----------------------------

/// Bounded LRU cache of egui thumbnails keyed by texture path.
///
/// The UI asks for a texture via [`ThumbnailCache::ensure`] while rendering a
/// cell. The bevy handle is stored as soon as the path is requested so the asset
/// load isn't cancelled by the handle being dropped, and once the asset has
/// finished loading the (downscaled) pixels are uploaded straight into the egui
/// context so the image shows up in the same frame. Dropping a
/// [`egui::TextureHandle`] frees the egui texture, so evicting a stale entry
/// past the cap reclaims its VRAM. Must be called every frame that uses the
/// cache, and `end_frame` runs the eviction pass.
#[derive(Resource, Default)]
pub struct ThumbnailCache {
    entries: HashMap<String, ThumbEntry>,
    order: VecDeque<String>,
    used: HashSet<String>,
}

struct ThumbEntry {
    /// Bevy handle; kept alive so the load finishes. Dropped on eviction.
    handle: Handle<Image>,
    /// egui texture, `None` while the bevy asset is still loading.
    texture: Option<egui::TextureHandle>,
}

impl ThumbnailCache {
    /// Get the egui texture id for `path`, uploading it first if the bevy asset
    /// has loaded. Returns `None` while the asset is still loading. Marks the
    /// path as used this frame so `end_frame` doesn't evict it.
    pub fn ensure(
        &mut self,
        ctx: &egui::Context,
        asset_server: &AssetServer,
        images: &Assets<Image>,
        path: &str,
    ) -> Option<egui::TextureId> {
        self.used.insert(path.to_string());
        if let Some(entry) = self.entries.get(path) {
            if let Some(texture) = &entry.texture {
                return Some(texture.id());
            }
        }
        let handle = match self.entries.get(path) {
            Some(entry) => entry.handle.clone(),
            None => {
                let handle = load_repeat(asset_server, path);
                self.entries.insert(path.to_string(), ThumbEntry { handle: handle.clone(), texture: None });
                self.order.push_back(path.to_string());
                handle
            }
        };
        let Some(image) = images.get(handle.id()) else {
            return None;
        };
        let Some(color) = image_to_color_image(image) else {
            return None;
        };
        let uploaded = ctx.load_texture(path.to_string(), color, egui::TextureOptions::LINEAR);
        if let Some(entry) = self.entries.get_mut(path) {
            entry.texture = Some(uploaded);
            return entry.texture.as_ref().map(|t| t.id());
        }
        None
    }

    /// Evict the oldest entries no longer visible this frame once the cap is
    /// exceeded. Call once per frame after the texture window has rendered.
    pub fn end_frame(&mut self) {
        while self.entries.len() > THUMB_CAP {
            let Some(evict) = self
                .order
                .iter()
                .find(|p| !self.used.contains(*p))
                .cloned()
            else {
                break;
            };
            self.order.retain(|p| p != &evict);
            self.entries.remove(&evict);
        }
        self.used.clear();
    }
}

/// Convert a loaded bevy image into an egui color image, downscaled to at most
/// 256px so thumbnails stay cheap on GPU memory. Only the default decode format
/// (`Rgba8UnormSrgb`, straight alpha) is handled; anything else yields `None`.
fn image_to_color_image(image: &Image) -> Option<egui::ColorImage> {
    const MAX_DIM: usize = 256;
    let format = image.texture_descriptor.format;
    if format != TextureFormat::Rgba8UnormSrgb && format != TextureFormat::Rgba8Unorm {
        return None;
    }
    let (w, h) = (image.width() as usize, image.height() as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let data = image.data.as_ref()?;
    if data.len() < w * h * 4 {
        return None;
    }
    let (dw, dh) = if w.max(h) > MAX_DIM {
        let scale = MAX_DIM as f32 / w.max(h) as f32;
        (
            ((w as f32) * scale).round().max(1.0) as usize,
            ((h as f32) * scale).round().max(1.0) as usize,
        )
    } else {
        (w, h)
    };
    let mut pixels = Vec::with_capacity(dw * dh);
    for j in 0..dh {
        let sy = ((j as f32 * h as f32) / dh as f32).floor() as usize;
        let row = sy * w;
        for i in 0..dw {
            let sx = ((i as f32 * w as f32) / dw as f32).floor() as usize;
            let p = (row + sx) * 4;
            pixels.push(egui::Color32::from_rgba_unmultiplied(
                data[p],
                data[p + 1],
                data[p + 2],
                data[p + 3],
            ));
        }
    }
    Some(egui::ColorImage {
        size: [dw, dh],
        source_size: egui::vec2(dw as f32, dh as f32),
        pixels,
    })
}

//------------------------------PLUGIN-------------------------------

pub struct TexturePlugin;

impl Plugin for TexturePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(scan_texture_catalog())
            .init_resource::<ThumbnailCache>();
    }
}

//------------------------------TESTS-------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_scan_is_sorted_and_assets_relative() {
        let catalog = scan_texture_catalog();
        if catalog.categories.is_empty() {
            // No TEXTURES dir present in this checkout; nothing to verify.
            return;
        }
        assert_eq!(catalog.categories.len(), catalog.paths.len());
        for (cat, paths) in catalog.categories.iter().zip(catalog.paths.iter()) {
            assert!(!paths.is_empty(), "category {cat} is empty");
            assert!(paths.windows(2).all(|w| w[0] < w[1]), "category {cat} not sorted");
            for p in paths {
                assert!(
                    p.starts_with(&format!("TEXTURES/{cat}/")),
                    "path {p} not under its category"
                );
            }
        }
    }

    #[test]
    fn catalog_categories_are_sorted() {
        let catalog = scan_texture_catalog();
        assert!(
            catalog.categories.windows(2).all(|w| w[0] < w[1]),
            "categories must be sorted"
        );
    }

    #[test]
    fn jpg_decode_panics_or_works() {
        use bevy::asset::RenderAssetUsages;
        use bevy::image::{CompressedImageFormats, ImageType};
        let bytes = std::fs::read("assets/TEXTURES/BAMBOO/BAM70001.jpg").unwrap();
        let img = Image::from_buffer(
            &bytes,
            ImageType::Extension("jpg"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::default(),
            RenderAssetUsages::default(),
        )
        .expect("Image::from_buffer decode");
        eprintln!(
            "decoded: {}x{} fmt={:?} len={:?}",
            img.width(),
            img.height(),
            img.texture_descriptor.format,
            img.data.as_ref().map(|d| d.len())
        );
    }
}
