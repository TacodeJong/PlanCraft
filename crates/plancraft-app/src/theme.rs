//! Colors, type and shapes of PlanCraft's ArtCraft-family look: near-black surfaces (or warm
//! paper in the light variant), 15 % hairlines, Archivo display type, Inter body text, Geist Mono
//! labels, 3 px corners on anything pressable and square modals.
//!
//! All drawing code reads colors from [`pal()`], the palette of the theme that is currently
//! shown. Hovers and selection chrome are translucent washes, never the accent; the accent
//! `#2d81ff` appears only as selection fill, focus rings, blue text tiers and a few filled marks.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use egui::epaint::text::VariationCoords;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, FontTweak, Stroke, TextStyle, Theme, ThemePreference, Visuals,
};
use serde::{Deserialize, Serialize};

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// `(r, g, b)` at alpha `a`, layering on whatever surface is below (the wash/hairline logic).
const fn wash(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(r, g, b, a)
}

/// Every color the UI paints with. Field names follow the old constant names.
#[allow(non_snake_case)]
#[derive(Clone, Copy)]
pub struct Palette {
    // Brand
    pub ACCENT: Color32,
    pub ACCENT_DARK: Color32,
    pub ACCENT_MID: Color32,
    pub ACCENT_LIGHT: Color32,
    pub ACCENT_PALE: Color32,
    pub TITLE_BAR: Color32,
    pub GOLD: Color32,
    // Surfaces & text
    pub TEXT: Color32,
    pub TEXT_WEAK: Color32,
    pub BG: Color32,
    pub PANEL_BG: Color32,
    pub HEADER_BG: Color32,
    pub INPUT_BG: Color32,
    /// Modals, menus and the top bar (ArtCraft's `ui-panel` tier).
    pub PANEL: Color32,
    /// Buttons, toolbar rows and the prompt box (`ui-controls`).
    pub CONTROLS: Color32,
    /// Wells and inputs (`ui-sunken`): the deepest tier, the brightest on paper.
    pub SUNKEN: Color32,
    /// Every hairline: white @ 15 % in the dark, black @ 15 % on paper.
    pub LINE: Color32,
    /// Outlined buttons and corner marks: ink @ 40 %.
    pub LINE_STRONG: Color32,
    /// Quiet fills (5 %).
    pub WASH: Color32,
    /// Selected rows (8 %).
    pub WASH_SELECTED: Color32,
    /// Hovered rows and buttons (10 %).
    pub WASH_HOVER: Color32,
    /// The primary block button: a white block with near-black text (inverted per theme).
    pub INVERT_BG: Color32,
    pub INVERT_FG: Color32,
    pub TRACK: Color32,
    pub SCROLL_THUMB: Color32,
    pub GRID_LINE: Color32,
    pub GRID_LINE_STRONG: Color32,
    pub SELECTION: Color32,
    pub SELECTION_CELL: Color32,
    pub SELECTION_TINT: Color32,
    pub NONWORKING: Color32,
    // Charts
    pub CHART_GRID: Color32,
    pub PROJECT_START: Color32,
    pub BAR_TASK: Color32,
    pub BAR_TASK_EDGE: Color32,
    pub BAR_PROGRESS: Color32,
    pub BAR_CRITICAL: Color32,
    pub BAR_CRITICAL_EDGE: Color32,
    pub BAR_SUMMARY: Color32,
    pub BAR_MANUAL: Color32,
    pub BAR_BASELINE: Color32,
    /// Bars of Baseline, Baseline1 … Baseline10 in the Multiple Baselines Gantt.
    pub BASELINE_SERIES: [Color32; 11],
    pub BAR_SLACK: Color32,
    pub BAR_INACTIVE: Color32,
    pub BAR_COMPLETE: Color32,
    pub MILESTONE: Color32,
    pub MEMBER_LINK: Color32,
    pub LINK: Color32,
    pub TODAY: Color32,
    pub STATUS_DATE: Color32,
    pub DEADLINE: Color32,
    pub TIMELINE_BAND: Color32,
    pub TIMELINE_TICK: Color32,
    pub TIMELINE_WINDOW: Color32,
    pub HEAT_LOW: Color32,
    pub HEAT_HIGH: Color32,
    pub OVER_LOW: Color32,
    pub OVER_HIGH: Color32,
    pub EXCEPTION_WORKING: Color32,
    pub EXCEPTION_OFF: Color32,
    // Status
    pub OK: Color32,
    pub WARN: Color32,
    pub ERROR: Color32,
    /// Status chips (Milestones view): text, and the Complete / Slipping / Late backgrounds.
    pub CHIP_TEXT: Color32,
    pub CHIP_OK: Color32,
    pub CHIP_WARN: Color32,
    pub CHIP_ERROR: Color32,
    /// The MCP server indicator on the title bar: running, stopped.
    pub MCP_LIVE: Color32,
    pub MCP_OFF: Color32,
}

/// The light variant, derived from the dark one: surfaces invert onto warm paper (tones from
/// ink `#f2f1ee`), hairlines and washes become black alphas, text-tier accent/status colors
/// darken to stay AA. Structure, geometry and accent fills stay the same.
pub const LIGHT: Palette = Palette {
    ACCENT: rgb(0x2D, 0x81, 0xFF),
    ACCENT_DARK: rgb(0x1A, 0x5B, 0xC8),
    ACCENT_MID: rgb(0x1C, 0x66, 0xDB),
    ACCENT_LIGHT: rgb(0xD8, 0xE4, 0xF5),
    ACCENT_PALE: rgb(0xE9, 0xF0, 0xFB),
    TITLE_BAR: rgb(0xE9, 0xE7, 0xE2),
    GOLD: rgb(0x8F, 0x5A, 0x00),
    TEXT: rgb(0x1D, 0x1D, 0x21),
    TEXT_WEAK: rgb(0x63, 0x62, 0x64),
    BG: rgb(0xF6, 0xF5, 0xF2),
    PANEL_BG: rgb(0xE9, 0xE7, 0xE2),
    HEADER_BG: rgb(0xEC, 0xEA, 0xE5),
    INPUT_BG: rgb(0xFF, 0xFF, 0xFF),
    PANEL: rgb(0xFB, 0xFA, 0xF8),
    CONTROLS: rgb(0xEC, 0xEA, 0xE5),
    SUNKEN: rgb(0xFF, 0xFF, 0xFF),
    LINE: wash(0x00, 0x00, 0x00, 38),
    LINE_STRONG: wash(0x1D, 0x1D, 0x21, 102),
    WASH: wash(0x00, 0x00, 0x00, 13),
    WASH_SELECTED: wash(0x00, 0x00, 0x00, 20),
    WASH_HOVER: wash(0x00, 0x00, 0x00, 26),
    INVERT_BG: rgb(0x14, 0x14, 0x18),
    INVERT_FG: rgb(0xF6, 0xF5, 0xF2),
    TRACK: rgb(0xE8, 0xE6, 0xE1),
    SCROLL_THUMB: rgb(0xB9, 0xB7, 0xB2),
    GRID_LINE: rgb(0xE4, 0xE2, 0xDD),
    GRID_LINE_STRONG: rgb(0xD5, 0xD3, 0xCE),
    SELECTION: rgb(0xD8, 0xE4, 0xF5),
    SELECTION_CELL: rgb(0xBD, 0xD4, 0xF2),
    SELECTION_TINT: Color32::from_rgba_premultiplied(0x5C, 0x62, 0x6C, 110),
    NONWORKING: rgb(0xEF, 0xED, 0xE9),
    CHART_GRID: rgb(0xEC, 0xEA, 0xE6),
    PROJECT_START: rgb(0xB8, 0xC4, 0xDE),
    BAR_TASK: rgb(0x5A, 0x86, 0xD6),
    BAR_TASK_EDGE: rgb(0x2F, 0x5B, 0xC4),
    BAR_PROGRESS: rgb(0x15, 0x2E, 0x73),
    BAR_CRITICAL: rgb(0xD9, 0x4F, 0x4F),
    BAR_CRITICAL_EDGE: rgb(0xA8, 0x2E, 0x2E),
    BAR_SUMMARY: rgb(0x33, 0x38, 0x45),
    BAR_MANUAL: rgb(0x9B, 0xBC, 0xEB),
    // Was #A9AEB8: 2.04:1 on the warm paper, darkened to keep the 3:1 glyph tier.
    BAR_BASELINE: rgb(0x80, 0x85, 0x90),
    BASELINE_SERIES: [
        rgb(0x8A, 0x90, 0x9C),
        rgb(0x1F, 0x8A, 0x8A),
        rgb(0xC8, 0x8A, 0x12),
        rgb(0x7B, 0x4F, 0xB8),
        rgb(0x3C, 0x8D, 0x3A),
        rgb(0xC0, 0x4A, 0x7A),
        rgb(0x2F, 0x6F, 0xC8),
        rgb(0xA0, 0x5A, 0x2C),
        rgb(0x5C, 0x7A, 0x1E),
        rgb(0x8E, 0x3B, 0x3B),
        rgb(0x4A, 0x55, 0x6A),
    ],
    BAR_SLACK: rgb(0x2C, 0x9C, 0x8F),
    BAR_INACTIVE: rgb(0xE6, 0xE4, 0xDF),
    BAR_COMPLETE: rgb(0x8F, 0xA3, 0xC8),
    MILESTONE: rgb(0x1B, 0x3A, 0x8C),
    MEMBER_LINK: rgb(0x9E, 0x75, 0x14),
    LINK: rgb(0x3C, 0x4A, 0x66),
    TODAY: rgb(0xBF, 0x69, 0x00),
    STATUS_DATE: rgb(0x8E, 0x44, 0xAD),
    DEADLINE: rgb(0x2E, 0x8B, 0x57),
    TIMELINE_BAND: rgb(0xE6, 0xE9, 0xF0),
    TIMELINE_TICK: rgb(0xC9, 0xD3, 0xE6),
    TIMELINE_WINDOW: Color32::from_rgba_premultiplied(0x03, 0x06, 0x0F, 28),
    HEAT_LOW: rgb(0xEC, 0xF2, 0xFC),
    HEAT_HIGH: rgb(0x8F, 0xB0, 0xE8),
    OVER_LOW: rgb(0xF6, 0xC6, 0xC6),
    OVER_HIGH: rgb(0xE0, 0x6A, 0x6A),
    EXCEPTION_WORKING: rgb(0xC9, 0xE8, 0xD8),
    EXCEPTION_OFF: rgb(0xF4, 0xC7, 0xC7),
    OK: rgb(0x1E, 0x7A, 0x4A),
    WARN: rgb(0xB4, 0x53, 0x09),
    ERROR: rgb(0xC0, 0x2B, 0x3C),
    CHIP_TEXT: rgb(0xFF, 0xFF, 0xFF),
    CHIP_OK: rgb(0x1E, 0x7A, 0x4A),
    CHIP_WARN: rgb(0xB4, 0x53, 0x09),
    CHIP_ERROR: rgb(0xC0, 0x2B, 0x3C),
    MCP_LIVE: rgb(0x1E, 0x7A, 0x4A),
    MCP_OFF: rgb(0x6E, 0x6D, 0x70),
};

/// ArtCraft's dark palette: five near-black surface tiers, one warm off-white ink with alpha
/// tiers, white hairlines and washes, accent only on selection and focus.
pub const DARK: Palette = Palette {
    ACCENT: rgb(0x2D, 0x81, 0xFF),
    ACCENT_DARK: rgb(0x74, 0xAA, 0xFF),
    ACCENT_MID: rgb(0x59, 0xA7, 0xFF),
    ACCENT_LIGHT: rgb(0x1A, 0x35, 0x5C),
    ACCENT_PALE: rgb(0x14, 0x23, 0x3C),
    TITLE_BAR: rgb(0x0A, 0x0A, 0x0C),
    GOLD: rgb(0xFF, 0xC1, 0x4D),
    TEXT: rgb(0xF2, 0xF1, 0xEE),
    TEXT_WEAK: rgb(0x9A, 0x99, 0x9B),
    BG: rgb(0x12, 0x13, 0x16),
    PANEL_BG: rgb(0x0A, 0x0A, 0x0C),
    HEADER_BG: rgb(0x1E, 0x1F, 0x22),
    INPUT_BG: rgb(0x06, 0x06, 0x07),
    PANEL: rgb(0x10, 0x10, 0x14),
    CONTROLS: rgb(0x1E, 0x1F, 0x22),
    SUNKEN: rgb(0x06, 0x06, 0x07),
    LINE: wash(0xFF, 0xFF, 0xFF, 38),
    LINE_STRONG: wash(0xF2, 0xF1, 0xEE, 102),
    WASH: wash(0xFF, 0xFF, 0xFF, 13),
    WASH_SELECTED: wash(0xFF, 0xFF, 0xFF, 20),
    WASH_HOVER: wash(0xFF, 0xFF, 0xFF, 26),
    INVERT_BG: rgb(0xFF, 0xFF, 0xFF),
    INVERT_FG: rgb(0x0B, 0x0B, 0x0C),
    TRACK: rgb(0x23, 0x24, 0x28),
    SCROLL_THUMB: rgb(0x47, 0x48, 0x4C),
    GRID_LINE: rgb(0x26, 0x27, 0x29),
    GRID_LINE_STRONG: rgb(0x35, 0x36, 0x39),
    SELECTION: rgb(0x1B, 0x2B, 0x47),
    SELECTION_CELL: rgb(0x23, 0x40, 0x69),
    SELECTION_TINT: Color32::from_rgba_premultiplied(0x10, 0x19, 0x2C, 110),
    NONWORKING: rgb(0x17, 0x18, 0x1C),
    CHART_GRID: rgb(0x1D, 0x1E, 0x22),
    PROJECT_START: rgb(0x4A, 0x58, 0x78),
    BAR_TASK: rgb(0x4F, 0x7C, 0xD0),
    BAR_TASK_EDGE: rgb(0x86, 0xA8, 0xEC),
    BAR_PROGRESS: rgb(0x10, 0x22, 0x55),
    BAR_CRITICAL: rgb(0xD9, 0x55, 0x55),
    BAR_CRITICAL_EDGE: rgb(0xF0, 0x8A, 0x8A),
    BAR_SUMMARY: rgb(0xC9, 0xCE, 0xDA),
    BAR_MANUAL: rgb(0x6F, 0x92, 0xD0),
    BAR_BASELINE: rgb(0x6B, 0x71, 0x80),
    BASELINE_SERIES: [
        rgb(0x9A, 0xA1, 0xAE),
        rgb(0x4F, 0xC1, 0xC1),
        rgb(0xE8, 0xB0, 0x40),
        rgb(0xA9, 0x86, 0xE6),
        rgb(0x72, 0xC2, 0x6E),
        rgb(0xE8, 0x7D, 0xA8),
        rgb(0x6E, 0xA4, 0xF0),
        rgb(0xD0, 0x8C, 0x5C),
        rgb(0xA8, 0xC8, 0x5A),
        rgb(0xD8, 0x7A, 0x7A),
        rgb(0xA8, 0xB2, 0xC6),
    ],
    BAR_SLACK: rgb(0x3F, 0xB8, 0xA8),
    BAR_INACTIVE: rgb(0x2A, 0x2B, 0x30),
    BAR_COMPLETE: rgb(0x5A, 0x6B, 0x8E),
    MILESTONE: rgb(0x86, 0xA8, 0xEC),
    MEMBER_LINK: rgb(0xD9, 0xA8, 0x3A),
    LINK: rgb(0x8C, 0x96, 0xAC),
    TODAY: rgb(0xF0, 0x93, 0x1E),
    STATUS_DATE: rgb(0xB3, 0x7F, 0xD6),
    DEADLINE: rgb(0x4C, 0xC3, 0x8A),
    TIMELINE_BAND: rgb(0x1A, 0x20, 0x30),
    TIMELINE_TICK: rgb(0x2E, 0x3A, 0x54),
    TIMELINE_WINDOW: Color32::from_rgba_premultiplied(0x0D, 0x16, 0x2F, 40),
    HEAT_LOW: rgb(0x1B, 0x24, 0x36),
    HEAT_HIGH: rgb(0x3A, 0x5F, 0xA8),
    OVER_LOW: rgb(0x46, 0x26, 0x2A),
    OVER_HIGH: rgb(0xA2, 0x3A, 0x40),
    EXCEPTION_WORKING: rgb(0x1E, 0x3A, 0x2C),
    EXCEPTION_OFF: rgb(0x46, 0x26, 0x2A),
    OK: rgb(0x22, 0xC5, 0x5E),
    WARN: rgb(0xFF, 0xC1, 0x4D),
    ERROR: rgb(0xF8, 0x71, 0x71),
    // Bright chips with near-black text: white on these light tints is unreadable.
    CHIP_TEXT: rgb(0x0F, 0x11, 0x15),
    CHIP_OK: rgb(0x22, 0xC5, 0x5E),
    CHIP_WARN: rgb(0xFF, 0xC1, 0x4D),
    CHIP_ERROR: rgb(0xF8, 0x71, 0x71),
    MCP_LIVE: rgb(0x4C, 0xC3, 0x8A),
    MCP_OFF: rgb(0x8A, 0x8A, 0x92),
};

static DARK_ACTIVE: AtomicBool = AtomicBool::new(true);
static CHART: AtomicU8 = AtomicU8::new(0);

const fn paint(
    mut p: Palette,
    bar: Color32,
    edge: Color32,
    summary: Color32,
    milestone: Color32,
    critical: Color32,
    critical_edge: Color32,
) -> Palette {
    p.BAR_TASK = bar;
    p.BAR_TASK_EDGE = edge;
    p.BAR_SUMMARY = summary;
    p.MILESTONE = milestone;
    p.BAR_CRITICAL = critical;
    p.BAR_CRITICAL_EDGE = critical_edge;
    p
}

const GRAPHITE_L: Palette = paint(
    LIGHT,
    rgb(0x6E, 0x74, 0x80),
    rgb(0x3E, 0x44, 0x50),
    rgb(0x2C, 0x30, 0x38),
    rgb(0x5C, 0x63, 0x70),
    rgb(0xB2, 0x3B, 0x3B),
    rgb(0x7A, 0x24, 0x24),
);
const GRAPHITE_D: Palette = paint(
    DARK,
    rgb(0x9A, 0xA1, 0xAD),
    rgb(0xD5, 0xD8, 0xDE),
    rgb(0xE6, 0xE8, 0xEC),
    rgb(0xC5, 0xCA, 0xD3),
    rgb(0xE0, 0x6A, 0x6A),
    rgb(0xF0, 0xA0, 0xA0),
);
const GREEN_L: Palette = paint(
    LIGHT,
    rgb(0x2F, 0x8F, 0x62),
    rgb(0x1B, 0x5C, 0x3E),
    rgb(0x16, 0x38, 0x2A),
    rgb(0x3E, 0x8F, 0x6A),
    rgb(0xC4, 0x48, 0x48),
    rgb(0x8E, 0x2E, 0x2E),
);
const GREEN_D: Palette = paint(
    DARK,
    rgb(0x4C, 0xAF, 0x7D),
    rgb(0x8E, 0xD4, 0xAE),
    rgb(0xD5, 0xE8, 0xDE),
    rgb(0x7D, 0xCC, 0xA0),
    rgb(0xE0, 0x6A, 0x6A),
    rgb(0xF0, 0xA0, 0xA0),
);
// Orange bars darken on the new paper background to keep the 3:1 glyph tier.
const ORANGE_L: Palette = paint(
    LIGHT,
    rgb(0xC8, 0x76, 0x1C),
    rgb(0x9E, 0x5A, 0x12),
    rgb(0x5C, 0x3A, 0x16),
    rgb(0xA8, 0x74, 0x10),
    rgb(0xC4, 0x48, 0x48),
    rgb(0x8E, 0x2E, 0x2E),
);
const ORANGE_D: Palette = paint(
    DARK,
    rgb(0xF0, 0xA2, 0x4A),
    rgb(0xF6, 0xC9, 0x8A),
    rgb(0xF3, 0xD7, 0xB0),
    rgb(0xF2, 0xC1, 0x4E),
    rgb(0xE0, 0x6A, 0x6A),
    rgb(0xF0, 0xA0, 0xA0),
);
const CONTRAST_L: Palette = paint(
    LIGHT,
    rgb(0x0B, 0x2E, 0x6B),
    rgb(0x00, 0x00, 0x00),
    rgb(0x00, 0x00, 0x00),
    rgb(0x00, 0x00, 0x00),
    rgb(0xB0, 0x00, 0x00),
    rgb(0x00, 0x00, 0x00),
);
const CONTRAST_D: Palette = paint(
    DARK,
    rgb(0xFF, 0xFF, 0xFF),
    rgb(0xFF, 0xE1, 0x4A),
    rgb(0xFF, 0xFF, 0xFF),
    rgb(0xFF, 0xE1, 0x4A),
    rgb(0xFF, 0x5A, 0x5A),
    rgb(0xFF, 0xFF, 0xFF),
);

/// The palette of the theme and design style currently on screen.
pub fn pal() -> &'static Palette {
    let dark = DARK_ACTIVE.load(Ordering::Relaxed);
    match (dark, CHART.load(Ordering::Relaxed)) {
        (false, 0) => &LIGHT,
        (true, 0) => &DARK,
        (false, 1) => &GRAPHITE_L,
        (true, 1) => &GRAPHITE_D,
        (false, 2) => &GREEN_L,
        (true, 2) => &GREEN_D,
        (false, 3) => &ORANGE_L,
        (true, 3) => &ORANGE_D,
        (false, 4) => &CONTRAST_L,
        (true, 4) => &CONTRAST_D,
        (true, _) => &DARK,
        (false, _) => &LIGHT,
    }
}

/// Remembers the design style for this process. Light and dark stay independent.
pub fn set_chart_style(style: plancraft_core::model::ChartStyle) {
    let n = match style {
        plancraft_core::model::ChartStyle::Blue => 0,
        plancraft_core::model::ChartStyle::Graphite => 1,
        plancraft_core::model::ChartStyle::Green => 2,
        plancraft_core::model::ChartStyle::Orange => 3,
        plancraft_core::model::ChartStyle::HighContrast => 4,
    };
    CHART.store(n, Ordering::Relaxed);
}

/// Swatches a group can wear. Views paint them through [`paint_rgb`].
pub const GROUP_SWATCHES: [[u8; 3]; 8] = [
    [0x5A, 0x86, 0xD6],
    [0x6E, 0x74, 0x80],
    [0x3E, 0x8F, 0x6A],
    [0xE0, 0x8A, 0x2F],
    [0xD9, 0x4F, 0x4F],
    [0xF2, 0xB1, 0x1C],
    [0xC4, 0x4B, 0x8B],
    [0x6A, 0x4C, 0xC4],
];

pub fn paint_rgb([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Amber, blue, pink and green row marks. Light and dark palettes both define these.
pub fn row_mark_color(mark: plancraft_core::model::RowMark) -> Color32 {
    match mark {
        plancraft_core::model::RowMark::Amber => pal().WARN,
        plancraft_core::model::RowMark::Blue => pal().ACCENT,
        plancraft_core::model::RowMark::Pink => pal().ERROR,
        plancraft_core::model::RowMark::Green => pal().CHIP_OK,
    }
}

/// Row tint for a task on the highlighted dependency chain. Direct links are stronger.
pub fn path_row(direct: bool) -> Color32 {
    let bg = pal().BG;
    let accent = pal().ACCENT;
    let t = if direct { 0.22 } else { 0.10 };
    let mix = |a: u8, b: u8| (f32::from(a) * (1.0 - t) + f32::from(b) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(mix(bg.r(), accent.r()), mix(bg.g(), accent.g()), mix(bg.b(), accent.b()))
}

/// The user's theme choice. The brutalist dark is the primary look.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    /// Follow the operating system.
    System,
    Light,
    #[default]
    Dark,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "System",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }

    pub fn parse(s: &str) -> Option<ThemeChoice> {
        ThemeChoice::ALL.into_iter().find(|t| t.label().eq_ignore_ascii_case(s.trim()))
    }

    fn preference(self) -> ThemePreference {
        match self {
            ThemeChoice::System => ThemePreference::System,
            ThemeChoice::Light => ThemePreference::Light,
            ThemeChoice::Dark => ThemePreference::Dark,
        }
    }
}

pub const ROW_HEIGHT: f32 = 22.0;
pub const HEADER_HEIGHT: f32 = 40.0;

/// The `.hud-label`: 11 px Geist Mono, upper case, widely spaced.
pub const HUD_SIZE: f32 = 11.0;
pub const HUD_SPACING: f32 = HUD_SIZE * 0.15;
/// Section labels and mono buttons are tracked a little tighter (0.12 em).
pub const LABEL_SPACING: f32 = HUD_SIZE * 0.12;

/// A 1 px hairline in the current theme's 15 % line color.
pub fn hairline() -> Stroke {
    Stroke::new(1.0, pal().LINE)
}

/// Inter 500: buttons and pill labels.
pub fn medium() -> FontFamily {
    FontFamily::Name("medium".into())
}

/// Inter 600: names and emphasized labels.
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// Archivo 620 at 118 % width: display headings.
pub fn display() -> FontFamily {
    FontFamily::Name("display".into())
}

/// Geist Mono 500: HUD labels.
pub fn mono() -> FontFamily {
    FontFamily::Name("mono".into())
}

/// Geist Mono 650: section labels and mono block-button labels.
pub fn mono_bold() -> FontFamily {
    FontFamily::Name("mono-bold".into())
}

/// Heat-map color for a utilization ratio (0 = idle, 1 = fully booked, >1 = overallocated).
pub fn utilization_color(u: f64) -> Color32 {
    let p = pal();
    if u <= 0.0 {
        return p.BG;
    }
    if u > 1.0001 {
        let t = ((u - 1.0) / 1.0).clamp(0.0, 1.0) as f32;
        return lerp(p.OVER_LOW, p.OVER_HIGH, t);
    }
    lerp(p.HEAT_LOW, p.HEAT_HIGH, u as f32)
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// The one shadow, under menus, modals and toasts only; flat panels get hairlines instead.
fn shadow(dark: bool) -> egui::epaint::Shadow {
    egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(if dark { 140 } else { 60 }) }
}

fn visuals(p: &Palette, dark: bool) -> Visuals {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    let ink_strong = if dark { Color32::WHITE } else { Color32::BLACK };
    v.panel_fill = p.BG;
    v.window_fill = p.PANEL;
    v.faint_bg_color = p.CONTROLS;
    v.extreme_bg_color = p.SUNKEN;
    v.code_bg_color = p.SUNKEN;
    v.text_edit_bg_color = Some(p.INPUT_BG);
    v.hyperlink_color = p.ACCENT_DARK;
    v.selection.bg_fill = p.ACCENT;
    v.selection.stroke = Stroke::new(1.0, ink_strong);
    v.text_cursor.stroke = Stroke::new(1.5, ink_strong);
    // Square modals; 3 px menus — pressables also get 3 px below.
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::same(3);
    v.window_stroke = Stroke::new(1.0, p.LINE);
    v.window_shadow = shadow(dark);
    v.popup_shadow = shadow(dark);
    v.override_text_color = None;
    v.warn_fg_color = p.WARN;
    v.error_fg_color = p.ERROR;
    v.striped = false;
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);
    let r = CornerRadius::same(3);
    // Idle controls sit on the CONTROLS tier with a hairline; hovers are washes with the
    // border raised to LINE_STRONG and the ink to full strength — never the accent.
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.TEXT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.LINE);
    v.widgets.noninteractive.bg_fill = p.BG;
    v.widgets.noninteractive.weak_bg_fill = p.BG;
    for (w, fill, stroke, fg) in [
        (&mut v.widgets.inactive, p.CONTROLS, p.LINE, p.TEXT),
        (&mut v.widgets.hovered, p.WASH_HOVER, p.LINE_STRONG, ink_strong),
        (&mut v.widgets.active, p.WASH_HOVER, p.LINE_STRONG, ink_strong),
        (&mut v.widgets.open, p.WASH_HOVER, p.LINE_STRONG, ink_strong),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
        w.fg_stroke = Stroke::new(1.0, fg);
        w.corner_radius = r;
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.expansion = 0.0;
    v
}

/// Installs PlanCraft's fonts and styles for both themes and applies the user's choice.
pub fn install(ctx: &egui::Context, choice: ThemeChoice) {
    install_fonts(ctx);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.0, medium())),
            (TextStyle::Heading, FontId::new(20.0, display())),
            (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
        ]
        .into();
        // Planner density: 22 px rows; the 34 px chrome controls are set locally.
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.interact_size.y = 22.0;
        style.spacing.menu_margin = egui::Margin::same(4);
    });
    ctx.style_mut_of(Theme::Light, |s| s.visuals = visuals(&LIGHT, false));
    ctx.style_mut_of(Theme::Dark, |s| s.visuals = visuals(&DARK, true));
    set_choice(ctx, choice);
}

/// The seven bundled faces, all SIL OFL 1.1 (license texts in `assets/fonts/`).
type Face<'a> = (&'a str, &'static [u8], &'a [([u8; 4], f32)], FontFamily);

/// Registers the three variable fonts at the weights (and width) the family look uses.
/// egui's own fonts stay behind ours for symbols our fonts lack, and the system-font
/// fallback (`fonts.rs`) appends scripts the bundle cannot draw.
fn install_fonts(ctx: &egui::Context) {
    const INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Variable.ttf");
    const ARCHIVO: &[u8] = include_bytes!("../assets/fonts/Archivo-Variable.ttf");
    const GEIST_MONO: &[u8] = include_bytes!("../assets/fonts/GeistMono-Variable.ttf");
    let mut fonts = FontDefinitions::default();
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let faces: [Face<'_>; 7] = [
        ("Inter", INTER, &[(*b"wght", 400.0), (*b"opsz", 14.0)], FontFamily::Proportional),
        ("Inter-Medium", INTER, &[(*b"wght", 500.0), (*b"opsz", 14.0)], medium()),
        ("Inter-SemiBold", INTER, &[(*b"wght", 600.0), (*b"opsz", 14.0)], semibold()),
        ("Archivo-Display", ARCHIVO, &[(*b"wght", 620.0), (*b"wdth", 118.0)], display()),
        ("GeistMono", GEIST_MONO, &[(*b"wght", 400.0)], FontFamily::Monospace),
        ("GeistMono-Medium", GEIST_MONO, &[(*b"wght", 500.0)], mono()),
        ("GeistMono-SemiBold", GEIST_MONO, &[(*b"wght", 650.0)], mono_bold()),
    ];
    for (name, bytes, coords, family) in faces {
        let tweak = FontTweak { coords: VariationCoords::new(coords.iter().copied()), ..Default::default() };
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes).tweak(tweak)));
        let mut stack = vec![name.to_owned()];
        stack.extend(fallback.iter().cloned());
        fonts.families.insert(family, stack);
    }
    ctx.set_fonts(fonts);
}

/// Switches between system, light and dark.
pub fn set_choice(ctx: &egui::Context, choice: ThemeChoice) {
    ctx.set_theme(choice.preference());
    sync(ctx);
}

/// Points [`pal()`] at the theme egui is currently showing (call once per frame,
/// so following the system theme updates live).
pub fn sync(ctx: &egui::Context) {
    DARK_ACTIVE.store(ctx.theme() == Theme::Dark, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::{CONTRAST_D, CONTRAST_L, DARK, GRAPHITE_D, GRAPHITE_L, GREEN_D, GREEN_L, LIGHT, ORANGE_D, ORANGE_L, Palette};
    use egui::Color32;

    /// WCAG relative luminance.
    fn luminance(c: Color32) -> f64 {
        let ch = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }

    /// `fg` (translucent allowed) composited over opaque `bg`, then the WCAG ratio.
    fn contrast(fg: Color32, bg: Color32) -> f64 {
        let a = fg.a() as f64 / 255.0;
        let over = |f: u8, b: u8| (f as f64 * a + b as f64 * (1.0 - a)).round() as u8;
        let fg = Color32::from_rgb(over(fg.r(), bg.r()), over(fg.g(), bg.g()), over(fg.b(), bg.b()));
        let (x, y) = (luminance(fg), luminance(bg));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// Every chrome pair of the style plan's contrast table, both themes: body text at 4.5:1
    /// (AA), glyphs, icons and bar fills at 3:1. Dark is ArtCraft's own palette; light is the
    /// derived paper variant with the four AA-corrected values.
    #[test]
    fn both_themes_keep_text_readable() {
        for (name, p) in [("light", &LIGHT), ("dark", &DARK)] {
            let p: &Palette = p;
            // Ink on every surface tier.
            for (what, bg) in [
                ("BG", p.BG),
                ("PANEL", p.PANEL),
                ("PANEL_BG", p.PANEL_BG),
                ("HEADER_BG", p.HEADER_BG),
                ("CONTROLS", p.CONTROLS),
                ("SUNKEN", p.SUNKEN),
                ("TITLE_BAR", p.TITLE_BAR),
            ] {
                assert!(contrast(p.TEXT, bg) >= 7.0, "{name}: TEXT on {what} is {:.2}:1", contrast(p.TEXT, bg));
            }
            // Secondary text (the MUTED tier) everywhere it sits.
            for (what, bg) in [("BG", p.BG), ("CONTROLS", p.CONTROLS), ("TITLE_BAR", p.TITLE_BAR), ("PANEL", p.PANEL)] {
                assert!(contrast(p.TEXT_WEAK, bg) >= 4.5, "{name}: TEXT_WEAK on {what} is {:.2}:1", contrast(p.TEXT_WEAK, bg));
            }
            // Selected rows keep readable ink.
            assert!(contrast(p.TEXT, p.SELECTION) >= 4.5, "{name}: TEXT on SELECTION");
            assert!(contrast(p.TEXT, p.SELECTION_CELL) >= 4.5, "{name}: TEXT on SELECTION_CELL");
            // Accent text tier (links, focus) is AA on page and wells.
            assert!(contrast(p.ACCENT_DARK, p.BG) >= 4.5, "{name}: ACCENT_DARK on BG is {:.2}:1", contrast(p.ACCENT_DARK, p.BG));
            assert!(contrast(p.ACCENT_DARK, p.SUNKEN) >= 4.5, "{name}: ACCENT_DARK on SUNKEN");
            // Accent is a fill (selection, rings, buttons with large/bold labels): glyph tier.
            assert!(contrast(Color32::WHITE, p.ACCENT) >= 3.0, "{name}: white on accent fills");
            assert!(contrast(p.ACCENT, p.BG) >= 3.0, "{name}: accent rings on BG");
            assert!(contrast(p.TEXT, p.ACCENT) >= 3.0, "{name}: ink over accent selection");
            // The primary block button.
            assert!(contrast(p.INVERT_FG, p.INVERT_BG) >= 7.0, "{name}: inverted button label");
            // Status text.
            assert!(contrast(p.ERROR, p.BG) >= 4.5, "{name}: error text is {:.2}:1", contrast(p.ERROR, p.BG));
            assert!(contrast(p.OK, p.BG) >= 4.5, "{name}: ok text is {:.2}:1", contrast(p.OK, p.BG));
            assert!(contrast(p.WARN, p.BG) >= 3.0, "{name}: warning glyphs");
            assert!(contrast(p.GOLD, p.BG) >= 3.0, "{name}: amber badges");
            // Hairlines must still be visible but are not text: no upper test, only presence.
            for (what, fg, bg) in [
                ("Complete chip", p.CHIP_TEXT, p.CHIP_OK),
                ("Slipping chip", p.CHIP_TEXT, p.CHIP_WARN),
                ("Late chip", p.CHIP_TEXT, p.CHIP_ERROR),
                ("On track chip", p.ACCENT_DARK, p.ACCENT_LIGHT),
            ] {
                assert!(contrast(fg, bg) >= 4.5, "{name}: {what} is {:.2}:1", contrast(fg, bg));
            }
            // The MCP dot is an indicator glyph on the title band (3:1 tier).
            assert!(contrast(p.MCP_LIVE, p.TITLE_BAR) >= 3.0, "{name}: MCP live");
            assert!(contrast(p.MCP_OFF, p.TITLE_BAR) >= 3.0, "{name}: MCP off");
            // Chart glyphs and lines against the page.
            for (what, c) in [
                ("LINK", p.LINK),
                ("TODAY", p.TODAY),
                ("MEMBER_LINK", p.MEMBER_LINK),
                ("BAR_SLACK", p.BAR_SLACK),
                ("DEADLINE", p.DEADLINE),
                ("STATUS_DATE", p.STATUS_DATE),
                ("BAR_BASELINE", p.BAR_BASELINE),
            ] {
                assert!(contrast(c, p.BG) >= 3.0, "{name}: {what} on BG is {:.2}:1", contrast(c, p.BG));
            }
        }
        // The light accent button fill carries a white label at AA (dark uses ArtCraft's own
        // large-label accent button, glyph tier above).
        assert!(contrast(Color32::WHITE, LIGHT.ACCENT_MID) >= 4.5, "light: white on accent button fill");
        // Washes and hairlines composite over their surfaces and must stay subtle but visible.
        for (name, p) in [("light", &LIGHT), ("dark", &DARK)] {
            assert!(contrast(p.LINE, p.BG) < 2.0, "{name}: hairlines stay quiet");
            assert!(contrast(p.WASH_HOVER, p.BG) < 1.6, "{name}: hover wash stays quiet");
        }
    }

    /// Every design style keeps its bars at the 3:1 glyph tier against the page.
    #[test]
    fn design_styles_keep_bars_visible() {
        for (name, p) in [
            ("blue light", &LIGHT),
            ("blue dark", &DARK),
            ("graphite light", &GRAPHITE_L),
            ("graphite dark", &GRAPHITE_D),
            ("green light", &GREEN_L),
            ("green dark", &GREEN_D),
            ("orange light", &ORANGE_L),
            ("orange dark", &ORANGE_D),
            ("high contrast light", &CONTRAST_L),
            ("high contrast dark", &CONTRAST_D),
        ] {
            let p: &Palette = p;
            for (what, c) in
                [("task bar", p.BAR_TASK), ("summary bar", p.BAR_SUMMARY), ("milestone", p.MILESTONE), ("critical bar", p.BAR_CRITICAL)]
            {
                assert!(contrast(c, p.BG) >= 3.0, "{name}: {what} is {:.2}:1", contrast(c, p.BG));
            }
        }
    }

    fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                sources(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    /// Icons are Lucide SVGs (`crate::icons`) now: no emoji or pictographic characters may
    /// appear in source code outside comments. The sole exception is the milestone mark '♦',
    /// kept as a text prefix where a milestone name appears inside a plain label; typography
    /// (… – — ‹ › quotes) lies outside the scanned ranges.
    #[test]
    fn ui_text_has_no_emoji_or_pictographs() {
        let mut files = Vec::new();
        sources(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        let mut problems = Vec::new();
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap();
            for (n, line) in text.lines().enumerate() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                for c in line.chars() {
                    let v = c as u32;
                    let pictograph = (0x1F000..=0x1FAFF).contains(&v)   // emoji and symbols
                        || (0x2190..=0x21FF).contains(&v)               // arrows
                        || (0x2300..=0x23FF).contains(&v)               // technical (watches, media keys)
                        || (0x25A0..=0x25FF).contains(&v)               // geometric shapes
                        || (0x2600..=0x27BF).contains(&v)               // misc symbols and dingbats
                        || (0x2900..=0x2BFF).contains(&v)               // more arrows and symbols
                        || v == 0xFE0F; // emoji variation selector
                    if pictograph && c != '♦' {
                        problems.push(format!("{}:{}: '{c}' (U+{v:04X})", f.display(), n + 1));
                    }
                }
            }
        }
        assert!(problems.is_empty(), "pictographs in source (use crate::icons instead):\n{}", problems.join("\n"));
    }

    /// The bundled fonts load with their variation coordinates on this egui and can lay text
    /// out in every family, including the five named faces.
    #[test]
    fn bundled_fonts_cover_all_seven_faces() {
        let ctx = egui::Context::default();
        super::install(&ctx, super::ThemeChoice::Dark);
        // Fonts exist only after the first frame.
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        for family in [
            egui::FontFamily::Proportional,
            egui::FontFamily::Monospace,
            super::medium(),
            super::semibold(),
            super::display(),
            super::mono(),
            super::mono_bold(),
        ] {
            let w = ctx.fonts_mut(|f| {
                f.layout_no_wrap("Projectplan 2026".into(), egui::FontId::new(13.0, family.clone()), Color32::WHITE).size().x
            });
            assert!(w > 10.0, "{family:?} lays out text");
        }
    }
}
