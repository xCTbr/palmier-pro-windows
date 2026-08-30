//! Text clips: style, animation, and word timing.

use serde::{Deserialize, Serialize};

use crate::codec::{
    DecodeError, Extra, FromObject, Object, PathStack, take_lenient, take_lenient_opt,
    take_object_lenient,
};
use crate::codec::{ObjectWriter, ToObject};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextFillMode {
    #[default]
    Color,
    Footage,
    Inverted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Alignment {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FontCase {
    #[default]
    Mixed,
    Uppercase,
    Lowercase,
}

/// All four channels required — synthesized decoding (research.md T006). Nested in
/// lenient parents, so a malformed colour usually collapses its parent to a default
/// rather than failing the load; nested in `TimelineMarker.color` it fails the load.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Rgba {
    pub const fn new(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r, g, b, a }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shadow {
    pub enabled: bool,
    pub color: Rgba,
    pub offset_x: f64,
    pub offset_y: f64,
    pub blur: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            enabled: false,
            color: Rgba::new(0.0, 0.0, 0.0, 1.0),
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 0.0,
            extra: Extra::new(),
        }
    }
}

/// Fully lenient (research.md T005).
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    pub enabled: bool,
    pub color: Rgba,
    pub width: f64,
    pub extra: Extra,
}

impl Default for Outline {
    fn default() -> Self {
        Self {
            enabled: false,
            color: Rgba::new(0.0, 0.0, 0.0, 1.0),
            width: 4.0,
            extra: Extra::new(),
        }
    }
}

impl FromObject for Outline {
    fn from_object(mut o: Object, _p: &mut PathStack) -> Result<Self, DecodeError> {
        let d = Self::default();
        Ok(Self {
            enabled: take_lenient(&mut o, "enabled", d.enabled),
            color: take_lenient(&mut o, "color", d.color),
            width: take_lenient(&mut o, "width", d.width),
            extra: o,
        })
    }
}

/// Fully lenient (research.md T005).
#[derive(Debug, Clone, PartialEq)]
pub struct Background {
    pub enabled: bool,
    pub color: Rgba,
    pub padding_x: f64,
    pub padding_y: f64,
    pub corner_radius: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub outline_color: Rgba,
    pub outline_width: f64,
    pub extra: Extra,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            enabled: false,
            color: Rgba::new(0.0, 0.0, 0.0, 0.6),
            padding_x: 0.0,
            padding_y: 0.0,
            corner_radius: 0.0,
            offset_x: 0.0,
            offset_y: 0.0,
            outline_color: Rgba::new(0.0, 0.0, 0.0, 1.0),
            outline_width: 0.0,
            extra: Extra::new(),
        }
    }
}

impl FromObject for Background {
    fn from_object(mut o: Object, _p: &mut PathStack) -> Result<Self, DecodeError> {
        let d = Self::default();
        Ok(Self {
            enabled: take_lenient(&mut o, "enabled", d.enabled),
            color: take_lenient(&mut o, "color", d.color),
            padding_x: take_lenient(&mut o, "paddingX", d.padding_x),
            padding_y: take_lenient(&mut o, "paddingY", d.padding_y),
            corner_radius: take_lenient(&mut o, "cornerRadius", d.corner_radius),
            offset_x: take_lenient(&mut o, "offsetX", d.offset_x),
            offset_y: take_lenient(&mut o, "offsetY", d.offset_y),
            outline_color: take_lenient(&mut o, "outlineColor", d.outline_color),
            outline_width: take_lenient(&mut o, "outlineWidth", d.outline_width),
            extra: o,
        })
    }
}

/// Fully lenient. The original's own comment: "Missing-key-tolerant decode — older
/// files pick up defaults for fields added later."
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub font_name: String,
    pub font_size: f64,
    pub font_scale: f64,
    pub width_scale: f64,
    pub height_scale: f64,
    pub tracking: f64,
    pub line_spacing: f64,
    pub font_case: FontCase,
    pub is_bold: bool,
    pub is_italic: bool,
    pub is_underlined: bool,
    pub is_struck_through: bool,
    pub is_overlined: bool,
    pub color: Rgba,
    pub alignment: Alignment,
    pub blur: f64,
    pub shadow: Shadow,
    pub background: Background,
    pub border: Outline,
    pub extra: Extra,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_name: "Helvetica".into(),
            font_size: 48.0,
            font_scale: 1.0,
            width_scale: 1.0,
            height_scale: 1.0,
            tracking: 0.0,
            line_spacing: 0.0,
            font_case: FontCase::default(),
            is_bold: false,
            is_italic: false,
            is_underlined: false,
            is_struck_through: false,
            is_overlined: false,
            color: Rgba::new(1.0, 1.0, 1.0, 1.0),
            alignment: Alignment::default(),
            blur: 0.0,
            shadow: Shadow::default(),
            background: Background::default(),
            border: Outline::default(),
            extra: Extra::new(),
        }
    }
}

/// The one corner of the format this project knowingly does not reproduce exactly.
///
/// When `isBold`/`isItalic` are absent the original infers them by constructing an
/// `NSFont` and reading `CTFontGetSymbolicTraits`, so the result depends on which
/// fonts are installed — the same file can decode differently on two Macs. This
/// approximates with a name-token heuristic. Files carrying the explicit keys decode
/// identically. See research.md T005.
fn infer_traits(font_name: &str) -> (bool, bool) {
    let n = font_name.to_ascii_lowercase();
    let bold = ["bold", "black", "heavy", "semibold", "demibold"]
        .iter()
        .any(|t| n.contains(t));
    let italic = ["italic", "oblique"].iter().any(|t| n.contains(t));
    (bold, italic)
}

impl FromObject for TextStyle {
    fn from_object(mut o: Object, p: &mut PathStack) -> Result<Self, DecodeError> {
        let d = Self::default();
        let font_name: String = take_lenient(&mut o, "fontName", d.font_name.clone());
        let (inferred_bold, inferred_italic) = infer_traits(&font_name);
        Ok(Self {
            font_size: take_lenient(&mut o, "fontSize", d.font_size),
            font_scale: take_lenient(&mut o, "fontScale", d.font_scale),
            width_scale: take_lenient(&mut o, "widthScale", d.width_scale),
            height_scale: take_lenient(&mut o, "heightScale", d.height_scale),
            tracking: take_lenient(&mut o, "tracking", d.tracking),
            line_spacing: take_lenient(&mut o, "lineSpacing", d.line_spacing),
            font_case: take_lenient(&mut o, "fontCase", d.font_case),
            is_bold: take_lenient(&mut o, "isBold", inferred_bold),
            is_italic: take_lenient(&mut o, "isItalic", inferred_italic),
            is_underlined: take_lenient(&mut o, "isUnderlined", d.is_underlined),
            is_struck_through: take_lenient(&mut o, "isStruckThrough", d.is_struck_through),
            is_overlined: take_lenient(&mut o, "isOverlined", d.is_overlined),
            color: take_lenient(&mut o, "color", d.color),
            alignment: take_lenient(&mut o, "alignment", d.alignment),
            blur: take_lenient(&mut o, "blur", d.blur),
            shadow: take_lenient(&mut o, "shadow", d.shadow),
            background: take_object_lenient(&mut o, "background", Background::default(), p),
            border: take_object_lenient(&mut o, "border", Outline::default(), p),
            font_name,
            extra: o,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextAnimationPreset {
    #[default]
    None,
    TypeOn,
    FadeIn,
    PopIn,
    SlideUp,
    Karaoke,
}

/// Nothing required.
#[derive(Debug, Clone, PartialEq)]
pub struct TextAnimation {
    pub preset: TextAnimationPreset,
    pub per_word_frames: i64,
    pub highlight: Option<Rgba>,
    pub extra: Extra,
}

impl FromObject for TextAnimation {
    fn from_object(mut o: Object, _p: &mut PathStack) -> Result<Self, DecodeError> {
        Ok(Self {
            preset: take_lenient(&mut o, "preset", TextAnimationPreset::None),
            per_word_frames: take_lenient(&mut o, "perWordFrames", 6),
            highlight: take_lenient_opt(&mut o, "highlight"),
            extra: o,
        })
    }
}

/// All three fields required — synthesized decoding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordTiming {
    pub text: String,
    pub start_frame: i64,
    pub end_frame: i64,
    #[serde(flatten)]
    pub extra: Extra,
}

impl ToObject for Outline {
    fn to_object(&self) -> Object {
        let mut w = ObjectWriter::new();
        w.put("enabled", &self.enabled)
            .put("color", &self.color)
            .put("width", &self.width)
            .extras(&self.extra);
        w.finish()
    }
}

impl ToObject for Background {
    fn to_object(&self) -> Object {
        let mut w = ObjectWriter::new();
        w.put("enabled", &self.enabled)
            .put("color", &self.color)
            .put("paddingX", &self.padding_x)
            .put("paddingY", &self.padding_y)
            .put("cornerRadius", &self.corner_radius)
            .put("offsetX", &self.offset_x)
            .put("offsetY", &self.offset_y)
            .put("outlineColor", &self.outline_color)
            .put("outlineWidth", &self.outline_width)
            .extras(&self.extra);
        w.finish()
    }
}

impl ToObject for TextStyle {
    fn to_object(&self) -> Object {
        let mut w = ObjectWriter::new();
        w.put("fontName", &self.font_name)
            .put("fontSize", &self.font_size)
            .put("fontScale", &self.font_scale)
            .put("widthScale", &self.width_scale)
            .put("heightScale", &self.height_scale)
            .put("tracking", &self.tracking)
            .put("lineSpacing", &self.line_spacing)
            .put("fontCase", &self.font_case)
            .put("isBold", &self.is_bold)
            .put("isItalic", &self.is_italic)
            .put("isUnderlined", &self.is_underlined)
            .put("isStruckThrough", &self.is_struck_through)
            .put("isOverlined", &self.is_overlined)
            .put("color", &self.color)
            .put("alignment", &self.alignment)
            .put("blur", &self.blur)
            .put("shadow", &self.shadow)
            .put_object("background", &self.background)
            .put_object("border", &self.border)
            .extras(&self.extra);
        w.finish()
    }
}

impl ToObject for TextAnimation {
    fn to_object(&self) -> Object {
        let mut w = ObjectWriter::new();
        w.put("preset", &self.preset)
            .put("perWordFrames", &self.per_word_frames)
            .put_opt("highlight", &self.highlight)
            .extras(&self.extra);
        w.finish()
    }
}
