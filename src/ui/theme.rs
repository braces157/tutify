use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Spotify,
    Amber,
    Matrix,
    Cyberpunk,
    Monochrome,
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub background: Color,
    pub surface: Color,
    pub surface_alt: Color,
    pub surface_selected: Color,
    pub text: Color,
    pub text_muted: Color,
    pub text_subtle: Color,
    pub border: Color,
    pub border_focus: Color,
    pub primary: Color,
    pub primary_soft: Color,
    pub on_primary: Color,
    pub status_warning: Color,
    pub status_warning_bg: Color,
    pub status_error: Color,
    pub status_error_bg: Color,
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
            Self::Spotify => "Spotify",
            Self::Amber => "Amber",
            Self::Matrix => "Matrix",
            Self::Cyberpunk => "Cyberpunk",
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
        self.palette().primary
    }

    pub fn palette(self) -> Palette {
        match self {
            Self::Spotify => Palette {
                background: Color::Rgb(9, 13, 11),
                surface: Color::Rgb(14, 20, 17),
                surface_alt: Color::Rgb(19, 27, 22),
                surface_selected: Color::Rgb(23, 39, 30),
                text: Color::Rgb(231, 237, 233),
                text_muted: Color::Rgb(143, 157, 148),
                text_subtle: Color::Rgb(91, 108, 98),
                border: Color::Rgb(37, 51, 43),
                border_focus: Color::Rgb(45, 183, 94),
                primary: Color::Rgb(30, 215, 96),
                primary_soft: Color::Rgb(83, 162, 108),
                on_primary: Color::Rgb(7, 17, 11),
                status_warning: Color::Rgb(222, 171, 79),
                status_warning_bg: Color::Rgb(46, 36, 18),
                status_error: Color::Rgb(226, 111, 112),
                status_error_bg: Color::Rgb(47, 23, 24),
            },
            Self::Amber => Palette {
                background: Color::Rgb(14, 12, 8),
                surface: Color::Rgb(22, 18, 11),
                surface_alt: Color::Rgb(30, 23, 12),
                surface_selected: Color::Rgb(43, 32, 14),
                text: Color::Rgb(239, 230, 209),
                text_muted: Color::Rgb(166, 151, 122),
                text_subtle: Color::Rgb(111, 96, 70),
                border: Color::Rgb(62, 48, 27),
                border_focus: Color::Rgb(202, 139, 43),
                primary: Color::Rgb(232, 162, 49),
                primary_soft: Color::Rgb(177, 124, 47),
                on_primary: Color::Rgb(21, 15, 6),
                status_warning: Color::Rgb(232, 162, 49),
                status_warning_bg: Color::Rgb(49, 35, 13),
                status_error: Color::Rgb(224, 109, 94),
                status_error_bg: Color::Rgb(48, 23, 18),
            },
            Self::Matrix => Palette {
                background: Color::Rgb(7, 13, 9),
                surface: Color::Rgb(11, 20, 14),
                surface_alt: Color::Rgb(14, 28, 18),
                surface_selected: Color::Rgb(16, 38, 23),
                text: Color::Rgb(218, 235, 223),
                text_muted: Color::Rgb(128, 157, 135),
                text_subtle: Color::Rgb(76, 108, 84),
                border: Color::Rgb(27, 55, 35),
                border_focus: Color::Rgb(44, 181, 88),
                primary: Color::Rgb(55, 207, 100),
                primary_soft: Color::Rgb(67, 153, 94),
                on_primary: Color::Rgb(5, 16, 8),
                status_warning: Color::Rgb(213, 169, 77),
                status_warning_bg: Color::Rgb(44, 34, 16),
                status_error: Color::Rgb(220, 104, 105),
                status_error_bg: Color::Rgb(45, 21, 22),
            },
            Self::Cyberpunk => Palette {
                background: Color::Rgb(8, 11, 19),
                surface: Color::Rgb(13, 18, 29),
                surface_alt: Color::Rgb(19, 26, 40),
                surface_selected: Color::Rgb(19, 39, 53),
                text: Color::Rgb(224, 234, 240),
                text_muted: Color::Rgb(137, 155, 168),
                text_subtle: Color::Rgb(82, 101, 116),
                border: Color::Rgb(37, 54, 70),
                border_focus: Color::Rgb(33, 165, 188),
                primary: Color::Rgb(36, 196, 217),
                primary_soft: Color::Rgb(77, 140, 158),
                on_primary: Color::Rgb(5, 15, 21),
                status_warning: Color::Rgb(218, 170, 78),
                status_warning_bg: Color::Rgb(46, 36, 18),
                status_error: Color::Rgb(224, 105, 111),
                status_error_bg: Color::Rgb(47, 22, 28),
            },
            Self::Monochrome => Palette {
                background: Color::Rgb(11, 11, 11),
                surface: Color::Rgb(18, 18, 18),
                surface_alt: Color::Rgb(25, 25, 25),
                surface_selected: Color::Rgb(38, 38, 38),
                text: Color::Rgb(234, 234, 234),
                text_muted: Color::Rgb(158, 158, 158),
                text_subtle: Color::Rgb(99, 99, 99),
                border: Color::Rgb(53, 53, 53),
                border_focus: Color::Rgb(187, 187, 187),
                primary: Color::Rgb(222, 222, 222),
                primary_soft: Color::Rgb(166, 166, 166),
                on_primary: Color::Rgb(15, 15, 15),
                status_warning: Color::Rgb(202, 202, 202),
                status_warning_bg: Color::Rgb(47, 47, 47),
                status_error: Color::Rgb(238, 238, 238),
                status_error_bg: Color::Rgb(57, 57, 57),
            },
        }
    }
}
