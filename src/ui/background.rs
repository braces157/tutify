use super::*;
use anyhow::{Context, Result};
use image::{DynamicImage, ImageReader, imageops::FilterType};
use std::path::{Path, PathBuf};

type Rgb = [u8; 3];
type Quad = [Rgb; 4];

#[derive(Default)]
pub(crate) struct BackgroundState {
    source: Option<PathBuf>,
    image: Option<DynamicImage>,
    raster_size: (u16, u16),
    raster: Vec<Quad>,
}

pub(crate) fn validate_image(path: &Path) -> Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("Cannot find background image {}", path.display()))?;
    ImageReader::open(&canonical)
        .with_context(|| format!("Cannot open background image {}", canonical.display()))?
        .with_guessed_format()
        .context("Cannot detect the background image format")?
        .decode()
        .with_context(|| format!("Cannot decode background image {}", canonical.display()))?;
    Ok(canonical)
}

pub(super) fn apply(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, theme: Theme) {
    if theme != Theme::Glass {
        return;
    }
    if app.config.native_glass {
        apply_native_transparency(frame, theme);
        return;
    }
    let source = app
        .config
        .background_image
        .as_deref()
        .map(PathBuf::from)
        .or_else(windows_wallpaper);
    let Some(source) = source else {
        return;
    };
    let area = frame.area();
    if !render.background.prepare(&source, area.width, area.height) {
        return;
    }

    let palette = theme.palette();
    let dim = app.config.background_dim.min(85);
    let width = area.width as usize;
    let buffer = frame.buffer_mut();
    for y in 0..area.height {
        for x in 0..area.width {
            let Some(cell) = buffer.cell_mut((area.x + x, area.y + y)) else {
                continue;
            };
            let (tint, tint_alpha): (Rgb, u8) =
                if cell.bg == palette.background || cell.bg == Color::Reset {
                    (rgb(palette.background), 14)
                } else if cell.bg == palette.surface {
                    (rgb(palette.surface), 23)
                } else if cell.bg == palette.surface_alt {
                    (rgb(palette.surface_alt), 31)
                } else {
                    continue;
                };
            let Some(samples) = render
                .background
                .raster
                .get(y as usize * width + x as usize)
            else {
                continue;
            };

            let shaded = samples.map(|sample| glass_pixel(sample, tint, dim, tint_alpha));

            if cell.symbol() == " " {
                let encoded = encode_quad(shaded);
                cell.set_symbol(encoded.symbol);
                cell.set_fg(color(encoded.foreground));
                cell.set_bg(color(encoded.background));
            } else {
                cell.set_bg(color(mean(&shaded, 0b1111)));
            }
        }
    }
}

fn apply_native_transparency(frame: &mut Frame<'_>, theme: Theme) {
    let palette = theme.palette();
    let area = frame.area();
    let buffer = frame.buffer_mut();
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let Some(cell) = buffer.cell_mut((x, y)) else {
                continue;
            };
            if cell.bg == palette.background
                || cell.bg == palette.surface
                || cell.bg == palette.surface_alt
            {
                cell.set_bg(Color::Reset);
            }
        }
    }
}

impl BackgroundState {
    fn prepare(&mut self, source: &Path, width: u16, height: u16) -> bool {
        if self.source.as_deref() != Some(source) {
            self.source = Some(source.to_owned());
            self.image = ImageReader::open(source)
                .ok()
                .and_then(|reader| reader.with_guessed_format().ok())
                .and_then(|reader| reader.decode().ok());
            self.raster_size = (0, 0);
            self.raster.clear();
        }
        let Some(image) = &self.image else {
            return false;
        };
        if self.raster_size == (width, height) {
            return !self.raster.is_empty();
        }
        // A typical terminal cell is roughly twice as tall as it is wide.  We sample
        // four quadrants per cell (2x2), but resize to a virtual 2x4 square-pixel
        // grid first so image geometry stays correct.  Each pair of virtual rows is
        // averaged into one quadrant.  This doubles horizontal detail compared with
        // a half-block renderer without stretching the source image.
        let pixel_width = u32::from(width).saturating_mul(2).max(1);
        let virtual_height = u32::from(height).saturating_mul(4).max(1);
        let resized = image
            .resize_to_fill(pixel_width, virtual_height, FilterType::Lanczos3)
            .to_rgb8();
        self.raster.clear();
        self.raster.reserve(width as usize * height as usize);
        for y in 0..height {
            for x in 0..width {
                let px = u32::from(x) * 2;
                let py = u32::from(y) * 4;
                self.raster.push([
                    average_pair(resized.get_pixel(px, py).0, resized.get_pixel(px, py + 1).0),
                    average_pair(
                        resized.get_pixel(px + 1, py).0,
                        resized.get_pixel(px + 1, py + 1).0,
                    ),
                    average_pair(
                        resized.get_pixel(px, py + 2).0,
                        resized.get_pixel(px, py + 3).0,
                    ),
                    average_pair(
                        resized.get_pixel(px + 1, py + 2).0,
                        resized.get_pixel(px + 1, py + 3).0,
                    ),
                ]);
            }
        }
        self.raster_size = (width, height);
        true
    }
}

fn windows_wallpaper() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("APPDATA")?)
        .join("Microsoft")
        .join("Windows")
        .join("Themes")
        .join("TranscodedWallpaper");
    path.is_file().then_some(path)
}

fn rgb(color: Color) -> [u8; 3] {
    match color {
        Color::Rgb(r, g, b) => [r, g, b],
        _ => [0, 0, 0],
    }
}

fn color(rgb: Rgb) -> Color {
    Color::Rgb(rgb[0], rgb[1], rgb[2])
}

fn average_pair(a: Rgb, b: Rgb) -> Rgb {
    [
        ((u16::from(a[0]) + u16::from(b[0])) / 2) as u8,
        ((u16::from(a[1]) + u16::from(b[1])) / 2) as u8,
        ((u16::from(a[2]) + u16::from(b[2])) / 2) as u8,
    ]
}

fn glass_pixel(source: Rgb, tint: Rgb, dim: u8, tint_alpha: u8) -> Rgb {
    let keep = 100u16.saturating_sub(u16::from(dim));
    let tint_alpha = u16::from(tint_alpha);
    let source_alpha = 100u16.saturating_sub(tint_alpha);
    let mut out = [0; 3];
    for channel in 0..3 {
        let darkened = u16::from(source[channel]) * keep / 100;
        out[channel] = ((darkened * source_alpha + u16::from(tint[channel]) * tint_alpha) / 100)
            .min(255) as u8;
    }
    out
}

struct EncodedQuad {
    symbol: &'static str,
    foreground: Rgb,
    background: Rgb,
}

/// Approximate four independently-colored quadrant samples with the two colors
/// a terminal cell can carry.  We test every unique two-way partition of the
/// 2x2 samples and choose the one with the smallest RGB squared error.
fn encode_quad(samples: Quad) -> EncodedQuad {
    let mut best_mask = 0u8;
    let mut best_error = u64::MAX;
    let mut best_foreground = mean(&samples, 0b1111);
    let mut best_background = best_foreground;

    // mask=0 is a single-color cell. Masks 1..=7 represent all unique
    // foreground/background partitions; 8..=14 are their complements.
    for mask in 0u8..=7 {
        let (foreground, background) = if mask == 0 {
            let average = mean(&samples, 0b1111);
            (average, average)
        } else {
            (mean(&samples, mask), mean(&samples, (!mask) & 0b1111))
        };
        let error = quantization_error(&samples, mask, foreground, background);
        if error < best_error {
            best_error = error;
            best_mask = mask;
            best_foreground = foreground;
            best_background = background;
        }
    }

    EncodedQuad {
        symbol: quadrant_symbol(best_mask),
        foreground: best_foreground,
        background: best_background,
    }
}

fn mean(samples: &Quad, mask: u8) -> Rgb {
    let mut total = [0u16; 3];
    let mut count = 0u16;
    for (index, sample) in samples.iter().enumerate() {
        if mask & (1 << index) == 0 {
            continue;
        }
        count += 1;
        for channel in 0..3 {
            total[channel] += u16::from(sample[channel]);
        }
    }
    if count == 0 {
        return [0, 0, 0];
    }
    [
        (total[0] / count) as u8,
        (total[1] / count) as u8,
        (total[2] / count) as u8,
    ]
}

fn quantization_error(samples: &Quad, mask: u8, foreground: Rgb, background: Rgb) -> u64 {
    let mut error = 0u64;
    for (index, sample) in samples.iter().enumerate() {
        let target = if mask != 0 && mask & (1 << index) != 0 {
            foreground
        } else {
            background
        };
        for channel in 0..3 {
            let delta = i32::from(sample[channel]) - i32::from(target[channel]);
            error += (delta * delta) as u64;
        }
    }
    error
}

fn quadrant_symbol(mask: u8) -> &'static str {
    match mask {
        0 => " ",
        1 => "▘",
        2 => "▝",
        3 => "▀",
        4 => "▖",
        5 => "▌",
        6 => "▞",
        7 => "▛",
        _ => " ",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quadrant_encoder_keeps_horizontal_detail() {
        let encoded = encode_quad([[240, 40, 40], [20, 40, 220], [230, 35, 35], [25, 35, 210]]);
        assert_eq!(encoded.symbol, "▌");
        assert!(encoded.foreground[0] > encoded.foreground[2]);
        assert!(encoded.background[2] > encoded.background[0]);
    }

    #[test]
    fn glass_pixel_dims_and_tints_source_reliably() {
        let source = [200, 200, 200];
        let tint = [10, 20, 30];
        let pixel = glass_pixel(source, tint, 30, 20);
        assert!(pixel[0] < 200);
        assert!(pixel[2] > pixel[0]);
    }
}
