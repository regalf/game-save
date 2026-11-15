//! The icon of a game, taken from its executable.
//!
//! The icon is embedded in the executable as a PE resource group. It is
//! read with a pure Rust parser, so it works on every platform: a Windows
//! game played with Wine on Linux gives its icon as well.
//!
//! The result is cached on disk, because reading the resources of a big
//! executable is not something to do again at every start.

use std::path::{Path, PathBuf};

use icoextract_rs::{IconExtractor, IconSelector};

/// An image, ready to be drawn.
#[derive(Debug, Clone)]
pub struct Icon {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The pixels, packed RGBA, from the top left.
    pub rgba: Vec<u8>,
}

impl Icon {
    /// Turn the image into something egui can draw.
    pub fn to_color_image(&self) -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied(
            [self.width as usize, self.height as usize],
            &self.rgba,
        )
    }
}

/// The magic of the cache file.
const MAGIC: &[u8; 4] = b"CSIC";

/// Read the icon of an executable.
///
/// `wanted` is the size one would like, in pixels. The closest size that the
/// executable carries is used instead.
pub fn from_executable(exe: &Path, wanted: u32) -> Option<Icon> {
    let extractor = IconExtractor::from_path(exe).ok()?;
    let group = extractor.get_icon(IconSelector::Index(0)).ok()?;
    let bytes = group.to_ico_bytes().ok()?;
    pick(&oxideav_ico::decode_all(&bytes).ok()?, wanted)
}

/// Read an icon from an icon file.
pub fn from_file(path: &Path, wanted: u32) -> Option<Icon> {
    let bytes = std::fs::read(path).ok()?;
    pick(&oxideav_ico::decode_all(&bytes).ok()?, wanted)
}

/// Pick the size that is the closest to the one wanted.
fn pick(frames: &[oxideav_ico::Frame], wanted: u32) -> Option<Icon> {
    if frames.is_empty() {
        return None;
    }

    // The closest size wins, the smallest one at least as big as the wanted
    // size is preferred over a bigger one.
    let best = frames.iter().min_by_key(|frame| {
        let width = frame.image.width;
        let too_small = width < wanted;
        (too_small, width.abs_diff(wanted))
    })?;

    let rgba = best.image.to_rgba8();
    if rgba.len() != (best.image.width * best.image.height * 4) as usize {
        return None;
    }

    Some(Icon {
        width: best.image.width,
        height: best.image.height,
        rgba,
    })
}

/// Load the icon of an executable, using the cache when it is there.
pub fn load(exe: &Path, cache: &Path, wanted: u32) -> Option<Icon> {
    if let Some(icon) = read_cache(cache) {
        return Some(icon);
    }
    let icon = from_executable(exe, wanted)?;
    let _ = write_cache(cache, &icon);
    Some(icon)
}

/// Read the cached icon.
fn read_cache(path: &Path) -> Option<Icon> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 13 || &bytes[..4] != MAGIC {
        return None;
    }
    let width = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    let height = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    let rgba = bytes[12..].to_vec();
    if rgba.len() != (width * height * 4) as usize {
        return None;
    }
    Some(Icon {
        width,
        height,
        rgba,
    })
}

/// Write the icon to the cache.
fn write_cache(path: &Path, icon: &Icon) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = Vec::with_capacity(12 + icon.rgba.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&icon.width.to_le_bytes());
    bytes.extend_from_slice(&icon.height.to_le_bytes());
    bytes.extend_from_slice(&icon.rgba);
    std::fs::write(path, bytes)
}

/// The path of the cached icon of a game.
pub fn cache_path(data_dir: &Path, slug: &str) -> PathBuf {
    data_dir.join("icons").join(format!("{slug}.icon"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An executable that is certainly there and certainly has an icon.
    #[cfg(target_os = "windows")]
    fn candidate() -> Option<PathBuf> {
        let windows = PathBuf::from(std::env::var_os("WINDIR")?);
        // Not every system file carries an icon: shell32.dll, for one, has
        // none. These three do.
        let candidates = [
            windows.join("explorer.exe"),
            windows.join("System32").join("cmd.exe"),
            windows.join("System32").join("notepad.exe"),
        ];
        candidates.into_iter().find(|path| path.exists())
    }

    #[cfg(not(target_os = "windows"))]
    fn candidate() -> Option<PathBuf> {
        None
    }

    #[test]
    fn the_icon_of_an_executable_is_read() {
        let Some(exe) = candidate() else {
            eprintln!("skipping: no executable with an icon at hand");
            return;
        };

        let icon = from_executable(&exe, 64).expect("the icon should be read");
        assert!(icon.width >= 16, "width = {}", icon.width);
        assert!(icon.height >= 16, "height = {}", icon.height);
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
        println!("{} -> {}x{}", exe.display(), icon.width, icon.height);
    }

    #[test]
    fn the_cache_round_trips() {
        let icon = Icon {
            width: 2,
            height: 2,
            rgba: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("test.icon");

        write_cache(&path, &icon).unwrap();
        let read = read_cache(&path).expect("the icon should be read back");
        assert_eq!(read.width, icon.width);
        assert_eq!(read.height, icon.height);
        assert_eq!(read.rgba, icon.rgba);
    }

    #[test]
    fn a_broken_cache_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.icon");
        std::fs::write(&path, b"not an icon at all").unwrap();
        assert!(read_cache(&path).is_none());
    }
}
