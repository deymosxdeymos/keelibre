use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyGroup {
    Alpha,
    Space,
    Enter,
    Backspace,
    Modifier,
    Tab,
    Arrow,
    Mouse,
}

impl KeyGroup {
    pub const ALL: [Self; 8] = [
        Self::Alpha,
        Self::Space,
        Self::Enter,
        Self::Backspace,
        Self::Modifier,
        Self::Tab,
        Self::Arrow,
        Self::Mouse,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Alpha => "alpha",
            Self::Space => "space",
            Self::Enter => "enter",
            Self::Backspace => "backspace",
            Self::Modifier => "modifier",
            Self::Tab => "tab",
            Self::Arrow => "arrow",
            Self::Mouse => "mouse",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct KeyPosition {
    pub group: KeyGroup,
    pub pan: f32,
    pub feel: f32,
}

impl Default for KeyPosition {
    fn default() -> Self {
        Self {
            group: KeyGroup::Alpha,
            pan: 0.0,
            feel: 1.0,
        }
    }
}

#[must_use]
pub fn lookup(code: u16) -> KeyPosition {
    use KeyGroup::{Alpha, Arrow, Backspace, Enter, Modifier, Space, Tab};
    let position = match code {
        14 | 111 => (Backspace, 0.95, 1.0),
        15 => (Tab, -0.90, 1.0),
        28 | 96 => (Enter, 0.95, 1.0),
        57 => (Space, 0.0, 1.0),
        103 | 108 => (Arrow, 0.80, 1.0),
        105 => (Arrow, 0.70, 1.0),
        106 => (Arrow, 0.95, 1.0),
        2..=11 => (Alpha, number_pan(code), 1.55),
        16 => (Alpha, -0.75, 1.0),
        17 => (Alpha, -0.60, 1.0),
        18 => (Alpha, -0.45, 1.0),
        19 => (Alpha, -0.30, 1.0),
        20 => (Alpha, -0.15, 1.3),
        21 | 48 => (Alpha, 0.0, 1.3),
        22 => (Alpha, 0.15, 1.0),
        23 => (Alpha, 0.30, 1.0),
        24 => (Alpha, 0.45, 1.0),
        25 => (Alpha, 0.60, 1.0),
        30 => (Alpha, -0.72, 0.4),
        31 => (Alpha, -0.55, 0.4),
        32 => (Alpha, -0.38, 0.4),
        33 => (Alpha, -0.20, 0.4),
        34 => (Alpha, -0.03, 1.3),
        35 => (Alpha, 0.12, 1.3),
        36 => (Alpha, 0.28, 0.4),
        37 => (Alpha, 0.45, 0.4),
        38 => (Alpha, 0.60, 0.4),
        44 => (Alpha, -0.70, 1.0),
        45 => (Alpha, -0.52, 1.0),
        46 => (Alpha, -0.35, 1.0),
        47 => (Alpha, -0.18, 1.0),
        49 => (Alpha, 0.15, 1.3),
        50 => (Alpha, 0.32, 1.0),
        51 => (Alpha, 0.48, 1.0),
        52 => (Alpha, 0.65, 1.0),
        1 | 29 | 42 | 54 | 56 | 58 | 59..=70 | 97 | 100 | 125..=127 => {
            (Modifier, modifier_pan(code), 1.0)
        }
        _ => return KeyPosition::default(),
    };
    KeyPosition {
        group: position.0,
        pan: position.1,
        feel: position.2,
    }
}

fn number_pan(code: u16) -> f32 {
    const PANS: [f32; 10] = [-0.8, -0.65, -0.5, -0.35, -0.2, -0.05, 0.1, 0.25, 0.4, 0.55];
    PANS[usize::from((code - 2) % 10)]
}

fn modifier_pan(code: u16) -> f32 {
    match code {
        29 | 42 | 56 => -0.8,
        54 | 97 | 100 => 0.9,
        125 => -0.65,
        126 | 127 => 0.65,
        59..=70 => -0.8 + f32::from(code - 59) * 1.75 / 11.0,
        _ => 0.0,
    }
}
