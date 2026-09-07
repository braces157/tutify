use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Spotify,
    Amber,
    Matrix,
    Cyberpunk,
    Monochrome,
}

impl Theme {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "amber" => Self::Amber,
            "matrix" => Self::Matrix,
            "cyberpunk" => Self::Cyberpunk,
            "monochrome" => Self::Monochrome,
            _ => Self::Spotify,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify Green",
            Self::Amber => "Amber CRT",
            Self::Matrix => "Matrix Green",
            Self::Cyberpunk => "Cyberpunk Cyan",
            Self::Monochrome => "Monochrome",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spotify => "spotify",
            Self::Amber => "amber",
            Self::Matrix => "matrix",
            Self::Cyberpunk => "cyberpunk",
            Self::Monochrome => "monochrome",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Spotify => Self::Amber,
            Self::Amber => Self::Matrix,
            Self::Matrix => Self::Cyberpunk,
            Self::Cyberpunk => Self::Monochrome,
            Self::Monochrome => Self::Spotify,
        }
    }
    pub fn primary(self) -> Color {
        match self {
            Self::Spotify => Color::Rgb(30, 215, 96),
            Self::Amber => Color::Rgb(255, 176, 0),
            Self::Matrix => Color::Rgb(0, 255, 102),
            Self::Cyberpunk => Color::Rgb(0, 229, 255),
            Self::Monochrome => Color::Rgb(240, 240, 240),
        }
    }
    pub fn highlight_bg(self) -> Color {
        match self {
            Self::Spotify => Color::Rgb(24, 45, 32),
            Self::Amber => Color::Rgb(50, 35, 10),
            Self::Matrix => Color::Rgb(10, 40, 20),
            Self::Cyberpunk => Color::Rgb(30, 20, 50),
            Self::Monochrome => Color::Rgb(40, 40, 40),
        }
    }
    pub fn border_inactive(self) -> Color {
        match self {
            Self::Spotify => Color::Rgb(40, 52, 45),
            Self::Amber => Color::Rgb(60, 45, 25),
            Self::Matrix => Color::Rgb(20, 50, 30),
            Self::Cyberpunk => Color::Rgb(40, 35, 65),
            Self::Monochrome => Color::Rgb(55, 55, 55),
        }
    }
    pub fn accent_dim(self) -> Color {
        match self {
            Self::Spotify => Color::Rgb(85, 160, 110),
            Self::Amber => Color::Rgb(180, 120, 20),
            Self::Matrix => Color::Rgb(40, 170, 75),
            Self::Cyberpunk => Color::Rgb(255, 0, 127),
            Self::Monochrome => Color::Rgb(160, 160, 160),
        }
    }
}
