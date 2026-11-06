//! How many colours the terminal draws, and the nearest of them to a colour, by arithmetic: this
//! runs for every coloured cell of a frame, so there's no list here to search.

/// The colours a terminal draws, most first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Truecolor,
    /// xterm's 6x6x6 cube and grey ramp (16-255): indexes below 16 are the terminal's scheme's.
    Colors256,
    /// ANSI 0-15, whatever the terminal's scheme makes of them.
    Colors16,
}

impl Tier {
    /// In the order of the `as u8` the player stores a tier by.
    pub const ALL: [Tier; 3] = [Tier::Truecolor, Tier::Colors256, Tier::Colors16];

    /// As `LOCALLIBRARY_COLOR` and the settings name it.
    pub fn parse(name: &str) -> Option<Tier> {
        match name.trim().to_ascii_lowercase().as_str() {
            "truecolor" | "24bit" => Some(Tier::Truecolor),
            "256" => Some(Tier::Colors256),
            "16" => Some(Tier::Colors16),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tier::Truecolor => "truecolor",
            Tier::Colors256 => "256",
            Tier::Colors16 => "16",
        }
    }
}

/// The build Terminal.app is counted as drawing 24-bit colour from: it started to with macOS 26, sets
/// no COLORTERM, and macOS 15's is 455.
const APPLE_TERMINAL_TRUECOLOR: u32 = 460;

/// What the environment says of a terminal that didn't say itself: 24-bit colour if `COLORTERM`
/// says so or it's a terminal known to draw it, else 256 colours if `TERM` says so or there's no
/// `TERM` (Windows), else 16.
pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Tier {
    let colorterm = var("COLORTERM").unwrap_or_default().to_lowercase();
    let program = var("TERM_PROGRAM").unwrap_or_default();
    let build = || var("TERM_PROGRAM_VERSION")?.split('.').next()?.parse::<u32>().ok();
    let truecolor = colorterm.contains("truecolor")
        || colorterm.contains("24bit")
        || matches!(program.as_str(), "iTerm.app" | "WezTerm" | "vscode" | "ghostty" | "Hyper" | "Tabby")
        || (program == "Apple_Terminal" && build().is_some_and(|b| b >= APPLE_TERMINAL_TRUECOLOR))
        || var("KITTY_WINDOW_ID").is_some()
        || var("WT_SESSION").is_some();
    if truecolor {
        return Tier::Truecolor;
    }
    match var("TERM") {
        Some(term) if !term.contains("256color") => Tier::Colors16,
        _ => Tier::Colors256,
    }
}

/// The channel values of xterm's colour cube.
const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Nearest colour in the xterm 6x6x6 cube or the grey ramp.
pub fn xterm256(r: u8, g: u8, b: u8) -> u8 {
    // Each level's reach ends halfway to the next.
    let nearest = |v: u8| match v {
        0..=47 => 0,
        48..=114 => 1,
        _ => (v as usize - 35) / 40,
    };
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let cube = [LEVELS[ri], LEVELS[gi], LEVELS[bi]];
    let dist = |c: [u8; 3]| -> i32 { c.iter().zip([r, g, b]).map(|(a, b)| (*a as i32 - b as i32).pow(2)).sum() };
    let avg = (r as u32 + g as u32 + b as u32) / 3;
    let step = ((avg.saturating_sub(8)) / 10).min(23) as u8;
    let grey = 8 + step * 10;
    if dist([grey, grey, grey]) < dist(cube) { 232 + step } else { (16 + 36 * ri + 6 * gi + bi) as u8 }
}

/// xterm's default colours for ANSI 0-15, which stand in for a scheme not known: a theme's roles
/// name their slots by these, and contrast is worked out on them.
pub const ANSI: [[u8; 3]; 16] = [
    [0, 0, 0],
    [205, 0, 0],
    [0, 205, 0],
    [205, 205, 0],
    [0, 0, 238],
    [205, 0, 205],
    [0, 205, 205],
    [229, 229, 229],
    [127, 127, 127],
    [255, 0, 0],
    [0, 255, 0],
    [255, 255, 0],
    [92, 92, 255],
    [255, 0, 255],
    [0, 255, 255],
    [255, 255, 255],
];

/// The ANSI colour of the same kind as `c`: a grey by how light it is, else which channels stand
/// out of it, bright when one is at full strength. Each of [`ANSI`] comes back as itself.
pub fn ansi16([r, g, b]: [u8; 3]) -> u8 {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max - min < 48 {
        return match max {
            0..=63 => 0,
            64..=170 => 8,
            171..=240 => 7,
            _ => 15,
        };
    }
    let middle = (max as u16 + min as u16) / 2;
    let on = |v: u8, bit: u8| if v as u16 >= middle { bit } else { 0 };
    on(r, 1) + on(g, 2) + on(b, 4) + if max >= 240 { 8 } else { 0 }
}

/// What a terminal of `tier` shows for `c`.
pub fn drawn(c: [u8; 3], tier: Tier) -> [u8; 3] {
    match tier {
        Tier::Truecolor => c,
        Tier::Colors256 => xterm_rgb(xterm256(c[0], c[1], c[2])),
        Tier::Colors16 => ANSI[usize::from(ansi16(c))],
    }
}

/// The colour xterm draws for an index of its cube or grey ramp (16-255).
fn xterm_rgb(i: u8) -> [u8; 3] {
    if i >= 232 {
        return [8 + 10 * (i - 232); 3];
    }
    let i = usize::from(i) - 16;
    [LEVELS[i / 36], LEVELS[i / 6 % 6], LEVELS[i % 6]]
}

#[cfg(test)]
mod tests;
