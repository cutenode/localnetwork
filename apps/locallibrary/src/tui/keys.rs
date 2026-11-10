//! Musical keys on the Camelot wheel, moved to the library ([`locallibrary::harmony`]) so the CLI,
//! the RPC sidecar and the set planner share it, and how the player writes and colours them.

pub use locallibrary::harmony::*;

/// How keys are written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Notation {
    /// `8A`.
    #[default]
    Camelot,
    /// `1m`, where 1m is A minor and 1d C major.
    OpenKey,
    /// `Am`.
    Musical,
}

impl Notation {
    pub const ALL: [Notation; 3] = [Notation::Camelot, Notation::OpenKey, Notation::Musical];

    /// As tui.json and the settings keep it.
    pub fn name(self) -> &'static str {
        match self {
            Notation::Camelot => "camelot",
            Notation::OpenKey => "open-key",
            Notation::Musical => "musical",
        }
    }

    pub fn parse(name: &str) -> Option<Notation> {
        Notation::ALL.into_iter().find(|n| n.name() == name)
    }

    /// `k` in this notation.
    pub fn write(self, k: Camelot) -> String {
        match self {
            Notation::Camelot => k.code(),
            Notation::OpenKey => format!("{}{}", open_key_number(k), if k.major { 'd' } else { 'm' }),
            Notation::Musical => k.note(),
        }
    }
}

/// `k`'s number in Open Key: 8A and 8B are 1.
fn open_key_number(k: Camelot) -> u8 {
    (k.number + 4) % 12 + 1
}

/// The colours keys come in. Each palette's twelve go round the wheel in Open Key's order, and the
/// ones made for colour blindness are Mixxx's (`predefinedcolorpalettes.cpp`): hue moves one way
/// round the circle and lightness the other, so each key differs from the rest while neighbours,
/// which mix, stay alike.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KeyPalette {
    /// One colour for all of them, the theme's.
    #[default]
    Theme,
    /// A colour wheel, for normal colour vision.
    Wheel,
    Protanopia,
    Deuteranopia,
    Tritanopia,
}

impl KeyPalette {
    pub const ALL: [KeyPalette; 5] = [
        KeyPalette::Theme,
        KeyPalette::Wheel,
        KeyPalette::Protanopia,
        KeyPalette::Deuteranopia,
        KeyPalette::Tritanopia,
    ];

    /// As tui.json and the settings keep it.
    pub fn name(self) -> &'static str {
        match self {
            KeyPalette::Theme => "theme",
            KeyPalette::Wheel => "wheel",
            KeyPalette::Protanopia => "protanopia",
            KeyPalette::Deuteranopia => "deuteranopia",
            KeyPalette::Tritanopia => "tritanopia",
        }
    }

    pub fn parse(name: &str) -> Option<KeyPalette> {
        KeyPalette::ALL.into_iter().find(|k| k.name() == name)
    }

    /// `k`'s colour: `None` for the theme's.
    pub fn colour(self, k: Camelot) -> Option<[u8; 3]> {
        let at = usize::from(open_key_number(k) - 1);
        let wheel = match self {
            KeyPalette::Theme => return None,
            KeyPalette::Wheel => &WHEEL,
            KeyPalette::Protanopia => &PROTANOPIA,
            KeyPalette::Deuteranopia => &DEUTERANOPIA,
            KeyPalette::Tritanopia => &TRITANOPIA,
        };
        Some(wheel[at])
    }
}

const WHEEL: [[u8; 3]; 12] = [
    [0xFC, 0x49, 0x49],
    [0xFE, 0x64, 0x2D],
    [0xF9, 0x8C, 0x27],
    [0xFE, 0xD6, 0x00],
    [0x99, 0xFE, 0x00],
    [0x42, 0xFE, 0x3E],
    [0x0A, 0xD5, 0x8F],
    [0x0A, 0xE7, 0xE7],
    [0x04, 0xC9, 0xFE],
    [0x3D, 0x8A, 0xFD],
    [0xAC, 0x64, 0xFE],
    [0xFD, 0x3F, 0xEA],
];

const PROTANOPIA: [[u8; 3]; 12] = [
    [0x26, 0x26, 0xD9],
    [0x75, 0x82, 0xD7],
    [0xA7, 0xC2, 0xDD],
    [0xB8, 0xE0, 0xE0],
    [0xA7, 0xDD, 0xC2],
    [0x75, 0xD7, 0x82],
    [0x26, 0xD9, 0x26],
    [0x0D, 0xA5, 0x22],
    [0x02, 0x78, 0x3D],
    [0x00, 0x66, 0x66],
    [0x02, 0x3D, 0x78],
    [0x0D, 0x22, 0xA5],
];

const DEUTERANOPIA: [[u8; 3]; 12] = [
    [0xD9, 0x26, 0x26],
    [0xD7, 0x75, 0x82],
    [0xDD, 0xA7, 0xC2],
    [0xE0, 0xB8, 0xE0],
    [0xC2, 0xA7, 0xDD],
    [0x82, 0x75, 0xD7],
    [0x26, 0x26, 0xD9],
    [0x22, 0x0D, 0xA5],
    [0x3D, 0x02, 0x78],
    [0x66, 0x00, 0x66],
    [0x78, 0x02, 0x3D],
    [0xA5, 0x0D, 0x22],
];

const TRITANOPIA: [[u8; 3]; 12] = [
    [0x26, 0xD9, 0x26],
    [0x82, 0xD7, 0x75],
    [0xC2, 0xDD, 0xA7],
    [0xE0, 0xE0, 0xB8],
    [0xDD, 0xC2, 0xA7],
    [0xD7, 0x82, 0x75],
    [0xD9, 0x26, 0x26],
    [0xA5, 0x22, 0x0D],
    [0x78, 0x3D, 0x02],
    [0x66, 0x66, 0x00],
    [0x3D, 0x78, 0x02],
    [0x22, 0xA5, 0x0D],
];
