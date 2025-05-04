use eframe::egui::ColorImage;
use eframe::egui::Context;
use eframe::egui::TextureHandle;
use eframe::egui::TextureOptions;
use image::RgbaImage;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::RwLock;

pub mod applications;
pub mod battery;
pub mod cpu;
pub mod date;
pub mod keyboard;
pub mod komorebi;
mod komorebi_layout;
pub mod media;
pub mod memory;
pub mod network;
pub mod storage;
pub mod time;
pub mod update;
pub mod widget;

/// Global cache for icon images and their associated GPU textures.
pub static ICONS_CACHE: IconsCache = IconsCache::new();

/// In-memory cache for icon images and their associated GPU textures.
///
/// Stores raw [`ColorImage`]s and [`TextureHandle`]s keyed by [`ImageIconId`].
/// Texture entries are context-dependent and automatically invalidated when the [`Context`] changes.
#[allow(clippy::type_complexity)]
pub struct IconsCache {
    textures: LazyLock<RwLock<(Option<Context>, HashMap<ImageIconId, TextureHandle>)>>,
    images: LazyLock<RwLock<HashMap<ImageIconId, Arc<ColorImage>>>>,
}

impl IconsCache {
    /// Creates a new empty IconsCache instance.
    #[inline]
    pub const fn new() -> Self {
        Self {
            textures: LazyLock::new(|| RwLock::new((None, HashMap::new()))),
            images: LazyLock::new(|| RwLock::new(HashMap::new())),
        }
    }

    /// Retrieves or creates a texture handle for the given icon ID and image.
    ///
    /// If a texture for the given ID already exists for the current [`Context`], it is reused.
    /// Otherwise, a new texture is created, inserted into the cache, and returned.
    /// The cache is reset if the [`Context`] has changed.
    #[inline]
    pub fn texture(&self, ctx: &Context, id: &ImageIconId, img: &Arc<ColorImage>) -> TextureHandle {
        if let Some(texture) = self.get_texture(ctx, id) {
            return texture;
        }
        let texture_handle = ctx.load_texture("icon", img.clone(), TextureOptions::default());
        self.insert_texture(ctx, id.clone(), texture_handle.clone());
        texture_handle
    }

    /// Returns the cached texture for the given icon ID if it exists and matches the current [`Context`].
    pub fn get_texture(&self, ctx: &Context, id: &ImageIconId) -> Option<TextureHandle> {
        let textures_lock = self.textures.read().unwrap();
        if textures_lock.0.as_ref() == Some(ctx) {
            return textures_lock.1.get(id).cloned();
        }
        None
    }

    /// Inserts a texture handle, resetting the cache if the [`Context`] has changed.
    pub fn insert_texture(&self, ctx: &Context, id: ImageIconId, texture: TextureHandle) {
        let mut textures_lock = self.textures.write().unwrap();

        if textures_lock.0.as_ref() != Some(ctx) {
            textures_lock.0 = Some(ctx.clone());
            textures_lock.1.clear();
        }

        textures_lock.1.insert(id, texture);
    }

    /// Returns the cached image for the given icon ID, if available.
    pub fn get_image(&self, id: &ImageIconId) -> Option<Arc<ColorImage>> {
        self.images.read().unwrap().get(id).cloned()
    }

    /// Caches a raw [`ColorImage`] associated with the given icon ID.
    pub fn insert_image(&self, id: ImageIconId, image: Arc<ColorImage>) {
        self.images.write().unwrap().insert(id, image);
    }
}

#[inline]
fn rgba_to_color_image(rgba_image: &RgbaImage) -> ColorImage {
    let size = [rgba_image.width() as usize, rgba_image.height() as usize];
    let pixels = rgba_image.as_flat_samples();
    ColorImage::from_rgba_unmultiplied(size, pixels.as_slice())
}

/// Represents an image-based icon with a unique ID and pixel data.
#[derive(Clone, Debug)]
pub struct ImageIcon {
    /// Unique identifier for the image icon, used for texture caching.
    pub id: ImageIconId,
    /// Shared pixel data of the icon in `ColorImage` format.
    pub image: Arc<ColorImage>,
}

impl ImageIcon {
    /// Creates a new [`ImageIcon`] from the given ID and image data.
    #[inline]
    pub fn new(id: ImageIconId, image: Arc<ColorImage>) -> Self {
        Self { id, image }
    }

    /// Loads an [`ImageIcon`] from [`ICONS_CACHE`] or calls `loader` if not cached.
    /// The loaded image is converted to a [`ColorImage`], cached, and returned.
    #[inline]
    pub fn try_load<F, I>(id: impl Into<ImageIconId>, loader: F) -> Option<Self>
    where
        F: FnOnce() -> Option<I>,
        I: Into<RgbaImage>,
    {
        let id = id.into();
        let image = ICONS_CACHE.get_image(&id).or_else(|| {
            let img = loader()?;
            let img = Arc::new(rgba_to_color_image(&img.into()));
            ICONS_CACHE.insert_image(id.clone(), img.clone());
            Some(img)
        })?;

        Some(ImageIcon::new(id, image))
    }

    /// Returns a texture handle for the icon, using the given [`Context`].
    ///
    /// If the texture is already cached in [`ICONS_CACHE`], it is reused.
    /// Otherwise, a new texture is created from the [`ColorImage`] and cached.
    #[inline]
    pub fn texture(&self, ctx: &Context) -> TextureHandle {
        ICONS_CACHE.texture(ctx, &self.id, &self.image)
    }
}

/// Unique identifier for an image-based icon.
///
/// Used to distinguish cached images and textures by either a file path
/// or a Windows window handle.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ImageIconId {
    /// Identifier based on a file system path.
    Path(Arc<Path>),
    /// Windows HWND handle.
    Hwnd(isize),
}

impl From<&Path> for ImageIconId {
    #[inline]
    fn from(value: &Path) -> Self {
        Self::Path(value.into())
    }
}

impl From<isize> for ImageIconId {
    #[inline]
    fn from(value: isize) -> Self {
        Self::Hwnd(value)
    }
}
