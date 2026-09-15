use crate::storage::Config;
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
    let mut styled = config.clone();
    styled.background_image = Some(prepare_background(&fragment, config)?);
    styled.background_dim = 0;
    write_fragment(&fragment, &styled, &executable)?;
    sync_terminal_settings(&styled)
}

// Bake the decorative treatment into the native image so text remains crisp.
fn prepare_background(fragment: &Path, config: &Config) -> Result<String> {
    let source = config
        .background_image
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| {
            Some(
                PathBuf::from(std::env::var_os("APPDATA")?)
                    .join("Microsoft/Windows/Themes/TranscodedWallpaper"),
            )
        })
        .context("Could not locate the wallpaper")?;
    let decoded = image::ImageReader::open(&source)?
        .with_guessed_format()?
        .decode()?;
    let mut image = decoded.to_rgb8();
    const TINT_COLOR: [f32; 3] = [14.0, 26.0, 30.0];
    let dim = (config.background_dim.clamp(0, 85) as f32) / 100.0;
    let base_dim = (dim * 0.50 + 0.30).clamp(0.25, 0.85);
    for (_, y, pixel) in image.enumerate_pixels_mut() {
        let scan = match y % 4 {
            0 => 0.35,
            1 => 0.65,
            2 => 0.90,
            _ => 0.65,
        };
        let art_luma =
            (0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32) / 255.0;
        let luma_boost = art_luma.powf(1.2) * 0.28;
        let eff_dim = (base_dim + (1.0 - base_dim) * luma_boost).clamp(0.0, 0.88);
        for (channel, &tint) in TINT_COLOR.iter().enumerate() {
            let art = pixel[channel] as f32;
            let dimmed = art * (1.0 - eff_dim) + tint * eff_dim;
            let scanned = dimmed * scan;
            pixel[channel] = scanned.clamp(0.0, 255.0).round() as u8;
        }
    }
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(image.as_raw());
    let output = fragment.with_file_name(format!("glass-{digest:x}.png"));
    if !output.exists() {
        image
            .save(&output)
            .context("Could not save Glass background")?;
    }
    Ok(output.to_string_lossy().into_owned())
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
    fn background_covers_full_window_and_adds_scanlines_at_pixel_resolution() {
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
        assert!(styled.get_pixel(0, 0)[0] < styled.get_pixel(0, 1)[0]);

        // Artwork covers full height rather than dissolving into solid background
        let bg_color = image::Rgb([8, 17, 21]);
        assert_ne!(styled.get_pixel(0, 60), &bg_color);
        assert_ne!(styled.get_pixel(0, 85), &bg_color);
        assert_ne!(styled.get_pixel(0, 119), &bg_color);

        // Scanline contrast between dark notch (row 0) and peak (row 2)
        let dark_notch = styled.get_pixel(0, 0)[0] as f32;
        let peak = styled.get_pixel(0, 2)[0] as f32;
        assert!(
            dark_notch / peak < 0.50,
            "scanline notches must produce deep CRT contrast"
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
