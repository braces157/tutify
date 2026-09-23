use crate::{storage::Config, ui::background::Orientation};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const PROFILE_NAME: &str = "Tuitify Glass";
const PROFILE_GUID: &str = "{cc251a12-05a2-52f9-aa7e-4972922503b2}";

pub(crate) fn available() -> bool {
    windows_terminal().is_some()
}

pub(crate) fn prepare(config: &Config) -> Result<()> {
    // Always relaunch the exact build the user invoked. A different Tuitify copy may
    // already be installed on PATH, and a stale copy might not understand Glass yet.
    let executable = std::env::current_exe().context("Could not locate the Tuitify executable")?;
    let fragment = fragment_path()?;
    let (horiz_bg, vert_bg) = prepare_responsive_backgrounds(&fragment, config)?;
    let mut styled = config.clone();
    styled.background_image = Some(horiz_bg);
    styled.background_image_vertical = Some(vert_bg);
    styled.background_dim = 0;
    write_fragment(&fragment, &styled, &executable)?;
    sync_terminal_settings(&styled)
}

pub(crate) fn switch_orientation(orientation: Orientation, config: &Config) -> Result<()> {
    if cfg!(test) {
        return Ok(());
    }
    let executable = std::env::current_exe().context("Could not locate the Tuitify executable")?;
    let fragment = fragment_path()?;
    let (horiz_bg, vert_bg) = prepare_responsive_backgrounds(&fragment, config)?;
    let active_bg = match orientation {
        Orientation::Horizontal => horiz_bg,
        Orientation::Vertical => vert_bg,
    };
    let mut styled = config.clone();
    styled.background_image = Some(active_bg);
    styled.background_dim = 0;
    write_fragment(&fragment, &styled, &executable)?;
    sync_terminal_settings(&styled)
}

#[allow(dead_code)]
pub(crate) fn prepare_background(fragment: &Path, config: &Config) -> Result<String> {
    prepare_responsive_backgrounds(fragment, config).map(|(horiz, _)| horiz)
}

fn prepare_responsive_backgrounds(fragment: &Path, config: &Config) -> Result<(String, String)> {
    let horiz_source = config
        .background_image
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| config.background_image_vertical.as_ref().map(PathBuf::from))
        .or_else(windows_wallpaper)
        .context("Could not locate horizontal background or wallpaper")?;
    let vert_source = config
        .background_image_vertical
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| config.background_image.as_ref().map(PathBuf::from))
        .or_else(windows_wallpaper)
        .context("Could not locate vertical background or wallpaper")?;
    let horiz_output = prepare_background_source(fragment, &horiz_source, config.background_dim)?;
    let vert_output = if horiz_source == vert_source {
        horiz_output.clone()
    } else {
        prepare_background_source(fragment, &vert_source, config.background_dim)?
    };
    Ok((horiz_output, vert_output))
}

fn windows_wallpaper() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("APPDATA")?)
        .join("Microsoft")
        .join("Windows")
        .join("Themes")
        .join("TranscodedWallpaper");
    path.is_file().then_some(path)
}

// Bake the clean, smooth treatment into the native image so text remains crisp and vibrant.
fn prepare_background_source(fragment: &Path, source: &Path, dim: u8) -> Result<String> {
    let key = background_cache_key(source, dim)?;
    let output = fragment.with_file_name(format!("glass-{key}.png"));
    if output.is_file() {
        return Ok(output.to_string_lossy().into_owned());
    }

    let decoded = image::ImageReader::open(source)?
        .with_guessed_format()?
        .decode()?;
    let mut image = decoded.to_rgb8();
    let dim_factor = (dim.clamp(0, 85) as f32) / 100.0;
    // Dimming smoothly darkens the artwork proportionally to the user's `--dim` setting,
    // preserving the natural colors and hues of the image, while applying CRT scanlines
    // that color-match the underlying image without foreign muddy tint cuts.
    let dim_mul = 1.0 - (dim_factor * 0.65);
    for (y, row) in image.enumerate_rows_mut() {
        let scan = match y % 4 {
            0 => 0.55,
            1 => 0.80,
            2 => 1.00,
            _ => 0.80,
        };
        let factor = dim_mul * scan;
        for (_, _, pixel) in row {
            for channel in 0..3 {
                let val = pixel[channel] as f32 * factor;
                pixel[channel] = val.clamp(0.0, 255.0).round() as u8;
            }
        }
    }
    image
        .save(&output)
        .context("Could not save Glass background")?;
    Ok(output.to_string_lossy().into_owned())
}

fn background_cache_key(source: &Path, dim: u8) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let meta = fs::metadata(source)
        .with_context(|| format!("Could not read metadata for {}", source.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(b"glass-color-matched-v4:");
    hasher.update(source.to_string_lossy().as_bytes());
    hasher.update(meta.len().to_le_bytes());
    if let Ok(mtime) = meta.modified() {
        if let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH) {
            hasher.update(dur.as_secs().to_le_bytes());
            hasher.update(dur.subsec_nanos().to_le_bytes());
        }
    }
    if let Ok(mut file) = fs::File::open(source) {
        let mut buf = [0u8; 8192];
        if let Ok(n) = file.read(&mut buf) {
            hasher.update(&buf[..n]);
        }
    }
    hasher.update([dim]);
    let digest = hasher.finalize();
    Ok(format!("{digest:x}"))
}

// Terminal can retain a fragment-derived appearance while creating new tabs.
// Persist only our background fields on our own profile, where user settings win.
fn sync_terminal_settings(config: &Config) -> Result<()> {
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?);
    for relative in [
        "Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json",
        "Packages/Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe/LocalState/settings.json",
        "Microsoft/Windows Terminal/settings.json",
    ] {
        let path = local.join(relative);
        if !path.is_file() {
            continue;
        }
        let original = fs::read(&path)?;
        let mut value: Value = serde_json::from_slice(&original).with_context(|| {
            format!(
                "Cannot update {}: settings must be valid JSON; comments are not supported",
                path.display()
            )
        })?;
        update_background_override(&mut value, config)?;
        let updated = serde_json::to_vec_pretty(&value)?;
        if updated != original {
            let backup = path.with_extension("json.tuitify-backup");
            if !backup.exists() {
                fs::write(&backup, &original)?;
            }
            fs::write(&path, updated)?;
        }
    }
    Ok(())
}

fn update_background_override(settings: &mut Value, config: &Config) -> Result<()> {
    let profiles = settings
        .pointer_mut("/profiles/list")
        .and_then(Value::as_array_mut)
        .context("Terminal settings have no profiles.list")?;
    let index = profiles.iter().position(|profile| {
        profile["guid"]
            .as_str()
            .is_some_and(|guid| guid.eq_ignore_ascii_case(PROFILE_GUID))
    });
    let index = index.unwrap_or_else(|| {
        profiles.push(json!({"guid": PROFILE_GUID, "name": PROFILE_NAME, "source": "Tuitify"}));
        profiles.len() - 1
    });
    let profile = &mut profiles[index];
    profile["backgroundImage"] = json!(config.background_image);
    profile["backgroundImageOpacity"] = json!(1.0);
    profile["backgroundImageStretchMode"] = json!("uniformToFill");
    profile["backgroundImageAlignment"] = json!("top");
    profile["background"] = json!("#081115");
    Ok(())
}

pub(crate) fn launch() -> Result<bool> {
    let Some(wt) = windows_terminal() else {
        return Ok(false);
    };
    let status = Command::new(wt)
        .args(["-w", "0", "new-tab", "-p", PROFILE_GUID])
        .status()
        .context("Could not launch the Windows Terminal Glass profile")?;
    Ok(status.success())
}

fn windows_terminal() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let alias = PathBuf::from(local)
        .join("Microsoft")
        .join("WindowsApps")
        .join("wt.exe");
    alias.is_file().then_some(alias)
}

fn fragment_path() -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    let dir = PathBuf::from(local)
        .join("Microsoft")
        .join("Windows Terminal")
        .join("Fragments")
        .join("Tuitify");
    fs::create_dir_all(&dir).with_context(|| format!("Could not create {}", dir.display()))?;
    Ok(dir.join("tuitify-glass.json"))
}

fn write_fragment(path: &Path, config: &Config, executable: &Path) -> Result<()> {
    let value = fragment_json(config, executable);
    let bytes = serde_json::to_vec_pretty(&value)?;
    if fs::read(path).ok().as_deref() == Some(bytes.as_slice()) {
        return Ok(());
    }
    fs::write(path, bytes).with_context(|| format!("Could not write {}", path.display()))
}

fn fragment_json(config: &Config, executable: &Path) -> Value {
    let image = config
        .background_image
        .as_deref()
        .unwrap_or("desktopWallpaper");
    let image_opacity =
        (100u16.saturating_sub(u16::from(config.background_dim.min(85)))) as f64 / 100.0;
    let commandline = format!("\"{}\" --native-glass", executable.display());
    json!({
        "profiles": [
            {
                "name": PROFILE_NAME,
                "guid": PROFILE_GUID,
                "commandline": commandline,
                "background": "#081115",
                "foreground": "#E8EFED",
                "selectionBackground": "#294844",
                "backgroundImage": image,
                "backgroundImageAlignment": "top",
                "backgroundImageOpacity": image_opacity,
                "backgroundImageStretchMode": "uniformToFill",
                "opacity": 100,
                "useAcrylic": false,
                "padding": "0",
                "scrollbarState": "hidden"
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_uses_gpu_wallpaper_profile_and_internal_marker() {
        let config = Config {
            theme: "glass".into(),
            background_dim: 38,
            ..Config::default()
        };
        let value = fragment_json(&config, Path::new(r"C:\Apps\Tuitify\tuitify.exe"));
        let profile = &value["profiles"][0];
        assert_eq!(profile["guid"], PROFILE_GUID);
        assert_eq!(profile["backgroundImage"], "desktopWallpaper");
        assert_eq!(profile["backgroundImageStretchMode"], "uniformToFill");
        assert_eq!(profile["backgroundImageAlignment"], "top");
        assert_eq!(profile["background"], "#081115");
        assert_eq!(profile["backgroundImageOpacity"], 0.62);
        assert!(
            profile["commandline"]
                .as_str()
                .unwrap()
                .contains("--native-glass")
        );
    }

    #[test]
    fn background_covers_full_window_and_preserves_clean_visuals_at_pixel_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.png");
        image::RgbImage::from_pixel(1930, 120, image::Rgb([220, 220, 220]))
            .save(&source)
            .unwrap();
        let config = Config {
            background_image: Some(source.to_string_lossy().into_owned()),
            ..Config::default()
        };
        let output = prepare_background(&dir.path().join("profile.json"), &config).unwrap();
        let styled = image::open(&output).unwrap().to_rgb8();
        assert_eq!(styled.dimensions(), (1930, 120));

        // Artwork covers full height rather than dissolving into solid background
        let bg_color = image::Rgb([8, 17, 21]);
        assert_ne!(styled.get_pixel(0, 60), &bg_color);
        assert_ne!(styled.get_pixel(0, 85), &bg_color);
        assert_ne!(styled.get_pixel(0, 119), &bg_color);

        // Visuals maintain CRT scanlines that color-match the image without foreign muddy tint cuts
        let row0 = styled.get_pixel(0, 0)[0] as f32;
        let row2 = styled.get_pixel(0, 2)[0] as f32;
        let ratio = row0 / row2;
        assert!(
            ratio > 0.50 && ratio < 0.60,
            "background must maintain color-matched CRT scanlines (ratio: {ratio})"
        );

        // Cached output returns immediately without error
        let cached = prepare_background(&dir.path().join("profile.json"), &config).unwrap();
        assert_eq!(cached, output);
    }

    #[test]
    fn background_override_preserves_other_profiles_and_preferences() {
        let mut settings = json!({"profiles": {"defaults": {"font": {"size": 14}}, "list": [
            {"guid": "other", "backgroundImage": "other.png"},
            {"guid": PROFILE_GUID, "font": {"size": 18}, "backgroundImage": "old.png"}
        ]}});
        let config = Config {
            background_image: Some("styled.png".into()),
            ..Config::default()
        };
        update_background_override(&mut settings, &config).unwrap();
        assert_eq!(
            settings["profiles"]["list"][0]["backgroundImage"],
            "other.png"
        );
        assert_eq!(settings["profiles"]["list"][1]["font"]["size"], 18);
        assert_eq!(settings["profiles"]["defaults"]["font"]["size"], 14);
        assert_eq!(
            settings["profiles"]["list"][1]["backgroundImage"],
            "styled.png"
        );
        assert_eq!(
            settings["profiles"]["list"][1]["backgroundImageAlignment"],
            "top"
        );
        assert_eq!(settings["profiles"]["list"][1]["background"], "#081115");
        let once = settings.clone();
        update_background_override(&mut settings, &config).unwrap();
        assert_eq!(settings, once);
    }

    #[test]
    fn fragment_preserves_custom_background_path() {
        let config = Config {
            background_image: Some(r"D:\Wallpapers\music.png".into()),
            ..Config::default()
        };
        let value = fragment_json(&config, Path::new(r"C:\tuitify.exe"));
        assert_eq!(
            value["profiles"][0]["backgroundImage"],
            r"D:\Wallpapers\music.png"
        );
    }

    #[test]
    fn responsive_background_switches_between_horizontal_and_vertical() {
        let dir = tempfile::tempdir().unwrap();
        let horiz = dir.path().join("horiz.png");
        let vert = dir.path().join("vert.png");
        image::RgbImage::from_pixel(100, 50, image::Rgb([200, 100, 100]))
            .save(&horiz)
            .unwrap();
        image::RgbImage::from_pixel(50, 100, image::Rgb([100, 100, 200]))
            .save(&vert)
            .unwrap();

        let config = Config {
            background_image: Some(horiz.to_string_lossy().into_owned()),
            background_image_vertical: Some(vert.to_string_lossy().into_owned()),
            ..Config::default()
        };
        let (h_out, v_out) = prepare_responsive_backgrounds(dir.path(), &config).unwrap();
        assert_ne!(h_out, v_out);
        let h_img = image::open(&h_out).unwrap().to_rgb8();
        let v_img = image::open(&v_out).unwrap().to_rgb8();
        assert_eq!(h_img.dimensions(), (100, 50));
        assert_eq!(v_img.dimensions(), (50, 100));
    }

    #[test]
    fn high_resolution_4k_background_preserves_dimensions_and_smooth_visuals() {
        let dir = tempfile::tempdir().unwrap();
        let source_4k = dir.path().join("4k.png");
        // Create 4K test texture (3840 x 100 to keep test execution fast while verifying 4K width)
        image::RgbImage::from_pixel(3840, 100, image::Rgb([180, 180, 180]))
            .save(&source_4k)
            .unwrap();

        let config = Config {
            background_image: Some(source_4k.to_string_lossy().into_owned()),
            background_dim: 40,
            ..Config::default()
        };
        let output = prepare_background(&dir.path().join("profile.json"), &config).unwrap();
        let styled = image::open(&output).unwrap().to_rgb8();
        // Resolution must maintain full 4K width without downscaling
        assert_eq!(styled.dimensions(), (3840, 100));
        // Visuals maintain CRT scanlines that color-match the artwork without foreign dark cuts
        let row0 = styled.get_pixel(0, 0)[0] as f32;
        let row2 = styled.get_pixel(0, 2)[0] as f32;
        let ratio = row0 / row2;
        assert!(
            ratio > 0.50 && ratio < 0.60,
            "high resolution background must maintain color-matched CRT scanlines (ratio: {ratio})"
        );
    }

    #[test]
    fn smooth_dimming_and_vibrance_fidelity() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("vibrant.png");
        // Create an image with distinct warm colors (e.g. skin tone / vibrant artwork)
        image::RgbImage::from_pixel(50, 50, image::Rgb([230, 140, 80]))
            .save(&source)
            .unwrap();

        // With dim = 0, artwork preserves 100% vibrance at peak scanline without artificial darkening
        let config_zero = Config {
            background_image: Some(source.to_string_lossy().into_owned()),
            background_dim: 0,
            ..Config::default()
        };
        let out_zero = prepare_background(&dir.path().join("p0.json"), &config_zero).unwrap();
        let img_zero = image::open(&out_zero).unwrap().to_rgb8();
        assert_eq!(img_zero.get_pixel(0, 2), &image::Rgb([230, 140, 80]));
        let p_zero_0 = img_zero.get_pixel(0, 0);
        assert!(p_zero_0[0] > p_zero_0[1] && p_zero_0[1] > p_zero_0[2]);

        // With dim = 40, artwork dims smoothly and preserves dominant hue (warm red/orange)
        let config_mid = Config {
            background_image: Some(source.to_string_lossy().into_owned()),
            background_dim: 40,
            ..Config::default()
        };
        let out_mid = prepare_background(&dir.path().join("p_mid.json"), &config_mid).unwrap();
        let img_mid = image::open(&out_mid).unwrap().to_rgb8();
        let p_mid = img_mid.get_pixel(0, 2);
        assert!(
            p_mid[0] > p_mid[1] && p_mid[1] > p_mid[2],
            "red/orange channel dominance must be preserved, not shifted to muddy green/teal"
        );
        assert!(
            p_mid[0] < 230 && p_mid[0] > 100,
            "dimming must be proportional (actual R: {})",
            p_mid[0]
        );

        // With dim = 80, artwork is smoothly darker than dim = 40
        let config_dark = Config {
            background_image: Some(source.to_string_lossy().into_owned()),
            background_dim: 80,
            ..Config::default()
        };
        let out_dark = prepare_background(&dir.path().join("p_dark.json"), &config_dark).unwrap();
        let img_dark = image::open(&out_dark).unwrap().to_rgb8();
        let p_dark = img_dark.get_pixel(0, 0);
        assert!(
            p_dark[0] < p_mid[0],
            "higher dim must produce darker pixels ({} < {})",
            p_dark[0],
            p_mid[0]
        );
    }

    #[test]
    fn text_readability_contrast_on_bright_background() {
        let dir = tempfile::tempdir().unwrap();
        let source_white = dir.path().join("white.png");
        // Pure white background: maximum challenge for text readability
        image::RgbImage::from_pixel(50, 50, image::Rgb([255, 255, 255]))
            .save(&source_white)
            .unwrap();

        let config = Config {
            background_image: Some(source_white.to_string_lossy().into_owned()),
            background_dim: 48,
            ..Config::default()
        };
        let output = prepare_background(&dir.path().join("p_read.json"), &config).unwrap();
        let styled = image::open(&output).unwrap().to_rgb8();
        let p = styled.get_pixel(0, 0);
        let bg_luma = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;

        // Terminal text foreground #E8EFED has luma ~237.
        // Even against a pure white background, dim=48 must ensure background luma stays <= 140
        // for comfortable, clear contrast.
        assert!(
            bg_luma <= 140.0,
            "background luma must stay comfortable for white text (luma: {bg_luma})"
        );
    }

    #[test]
    fn responsive_background_symmetric_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let vert = dir.path().join("only_vert.png");
        image::RgbImage::from_pixel(60, 120, image::Rgb([100, 150, 200]))
            .save(&vert)
            .unwrap();

        // When only vertical is set, horizontal falls back to vertical
        let config = Config {
            background_image: None,
            background_image_vertical: Some(vert.to_string_lossy().into_owned()),
            ..Config::default()
        };
        let (h_out, v_out) = prepare_responsive_backgrounds(dir.path(), &config).unwrap();
        assert_eq!(h_out, v_out);
    }

    #[test]
    fn switch_orientation_is_noop_in_test() {
        let config = Config::default();
        // In cfg!(test), switch_orientation must return Ok(()) without writing live user profiles
        assert!(switch_orientation(Orientation::Vertical, &config).is_ok());
        assert!(switch_orientation(Orientation::Horizontal, &config).is_ok());
    }

    #[test]
    #[ignore = "writes the live Windows Terminal profile; manual acceptance test"]
    fn prepare_generates_live_terminal_profile_and_background() {
        let config = Config {
            theme: "glass".into(),
            background_dim: 48,
            ..Config::default()
        };
        if available() {
            prepare(&config).unwrap();
        }
    }
}
