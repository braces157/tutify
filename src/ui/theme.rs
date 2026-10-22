use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Spotify,
    Amber,
    Matrix,
    Cyberpunk,
    Monochrome,
    Glass,
}

/// Semantic colour roles.
///
/// The set is deliberately small: one ground, two surface steps, three ink
/// steps, one accent family, and two status pairs.  `divider` is the hairline
/// rule colour and `track` is the unfilled part of a progress bar; both are
/// ambient and must never carry meaning on their own.
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
    pub divider: Color,
    pub track: Color,
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
            "glass" => Self::Glass,
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
            Self::Glass => "Glass",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spotify => "spotify",
            Self::Amber => "amber",
            Self::Matrix => "matrix",
            Self::Cyberpunk => "cyberpunk",
            Self::Monochrome => "monochrome",
            Self::Glass => "glass",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Spotify => Self::Amber,
            Self::Amber => Self::Matrix,
            Self::Matrix => Self::Cyberpunk,
            Self::Cyberpunk => Self::Monochrome,
            Self::Monochrome => Self::Glass,
            Self::Glass => Self::Spotify,
        }
    }

    pub fn primary(self) -> Color {
        self.palette().primary
    }

    pub fn palette(self) -> Palette {
        match self {
            Self::Spotify => Palette {
                background: Color::Rgb(16, 17, 19),
                surface: Color::Rgb(23, 25, 27),
                surface_alt: Color::Rgb(30, 32, 34),
                surface_selected: Color::Rgb(35, 53, 44),
                text: Color::Rgb(242, 243, 242),
                text_muted: Color::Rgb(176, 183, 180),
                text_subtle: Color::Rgb(132, 142, 137),
                border: Color::Rgb(61, 66, 63),
                border_focus: Color::Rgb(113, 210, 156),
                divider: Color::Rgb(39, 43, 41),
                track: Color::Rgb(65, 73, 68),
                primary: Color::Rgb(127, 224, 171),
                primary_soft: Color::Rgb(103, 177, 135),
                on_primary: Color::Rgb(12, 25, 17),
                status_warning: Color::Rgb(222, 171, 79),
                status_warning_bg: Color::Rgb(46, 36, 18),
                status_error: Color::Rgb(226, 111, 112),
                status_error_bg: Color::Rgb(47, 23, 24),
            },
            Self::Amber => Palette {
                background: Color::Rgb(13, 11, 7),
                surface: Color::Rgb(21, 17, 10),
                surface_alt: Color::Rgb(29, 23, 12),
                surface_selected: Color::Rgb(45, 34, 15),
                text: Color::Rgb(240, 231, 210),
                text_muted: Color::Rgb(172, 156, 125),
                text_subtle: Color::Rgb(119, 103, 74),
                border: Color::Rgb(66, 54, 32),
                border_focus: Color::Rgb(202, 139, 43),
                divider: Color::Rgb(40, 31, 17),
                track: Color::Rgb(56, 44, 26),
                primary: Color::Rgb(232, 162, 49),
                primary_soft: Color::Rgb(177, 124, 47),
                on_primary: Color::Rgb(21, 15, 6),
                status_warning: Color::Rgb(232, 162, 49),
                status_warning_bg: Color::Rgb(49, 35, 13),
                status_error: Color::Rgb(224, 109, 94),
                status_error_bg: Color::Rgb(48, 23, 18),
            },
            Self::Matrix => Palette {
                background: Color::Rgb(6, 12, 8),
                surface: Color::Rgb(11, 19, 14),
                surface_alt: Color::Rgb(15, 26, 18),
                surface_selected: Color::Rgb(17, 38, 24),
                text: Color::Rgb(216, 234, 221),
                text_muted: Color::Rgb(132, 160, 138),
                text_subtle: Color::Rgb(84, 115, 90),
                border: Color::Rgb(30, 57, 37),
                border_focus: Color::Rgb(44, 181, 88),
                divider: Color::Rgb(20, 37, 24),
                track: Color::Rgb(32, 58, 38),
                primary: Color::Rgb(55, 207, 100),
                primary_soft: Color::Rgb(67, 153, 94),
                on_primary: Color::Rgb(5, 16, 8),
                status_warning: Color::Rgb(213, 169, 77),
                status_warning_bg: Color::Rgb(44, 34, 16),
                status_error: Color::Rgb(220, 104, 105),
                status_error_bg: Color::Rgb(45, 21, 22),
            },
            Self::Cyberpunk => Palette {
                background: Color::Rgb(7, 10, 17),
                surface: Color::Rgb(12, 17, 27),
                surface_alt: Color::Rgb(18, 25, 38),
                surface_selected: Color::Rgb(19, 38, 51),
                text: Color::Rgb(223, 233, 240),
                text_muted: Color::Rgb(141, 158, 171),
                text_subtle: Color::Rgb(90, 108, 122),
                border: Color::Rgb(40, 56, 72),
                border_focus: Color::Rgb(33, 165, 188),
                divider: Color::Rgb(25, 35, 48),
                track: Color::Rgb(34, 48, 63),
                primary: Color::Rgb(36, 196, 217),
                primary_soft: Color::Rgb(77, 140, 158),
                on_primary: Color::Rgb(5, 15, 21),
                status_warning: Color::Rgb(218, 170, 78),
                status_warning_bg: Color::Rgb(46, 36, 18),
                status_error: Color::Rgb(224, 105, 111),
                status_error_bg: Color::Rgb(47, 22, 28),
            },
            Self::Monochrome => Palette {
                background: Color::Rgb(10, 10, 10),
                surface: Color::Rgb(17, 17, 17),
                surface_alt: Color::Rgb(24, 24, 24),
                surface_selected: Color::Rgb(39, 39, 39),
                text: Color::Rgb(234, 234, 234),
                text_muted: Color::Rgb(160, 160, 160),
                text_subtle: Color::Rgb(104, 104, 104),
                border: Color::Rgb(58, 58, 58),
                border_focus: Color::Rgb(187, 187, 187),
                divider: Color::Rgb(36, 36, 36),
                track: Color::Rgb(48, 48, 48),
                primary: Color::Rgb(222, 222, 222),
                primary_soft: Color::Rgb(166, 166, 166),
                on_primary: Color::Rgb(15, 15, 15),
                status_warning: Color::Rgb(202, 202, 202),
                status_warning_bg: Color::Rgb(47, 47, 47),
                status_error: Color::Rgb(238, 238, 238),
                status_error_bg: Color::Rgb(57, 57, 57),
            },
            Self::Glass => Palette {
                background: Color::Rgb(8, 16, 19),
                surface: Color::Rgb(12, 22, 25),
                surface_alt: Color::Rgb(17, 29, 32),
                surface_selected: Color::Rgb(31, 50, 52),
                text: Color::Rgb(246, 251, 250),
                text_muted: Color::Rgb(214, 232, 230),
                text_subtle: Color::Rgb(170, 196, 194),
                border: Color::Rgb(58, 78, 82),
                border_focus: Color::Rgb(140, 212, 198),
                divider: Color::Rgb(36, 52, 56),
                track: Color::Rgb(48, 66, 70),
                primary: Color::Rgb(164, 238, 222),
                primary_soft: Color::Rgb(152, 226, 212),
                on_primary: Color::Rgb(8, 22, 22),
                status_warning: Color::Rgb(231, 187, 93),
                status_warning_bg: Color::Rgb(52, 42, 23),
                status_error: Color::Rgb(239, 127, 132),
                status_error_bg: Color::Rgb(54, 27, 31),
            },
        }
    }
}
