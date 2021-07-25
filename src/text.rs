use std::{borrow::BorrowMut, convert::TryFrom, ops::DerefMut, sync::Arc};

use fontster::{Font, GlyphPosition, HorizontalAlign, Layout, LayoutSettings, StyledText};
use small_http::{Parameter, Query};
use smol::lock::{Mutex, RwLock};
use thiserror::Error;

use crate::{
    color::Color,
    image::{ColorProvider, Colors, Image, Mask, Stripes},
    FontProvider,
};

#[derive(Clone)]
pub enum Visual {
    Color(Color),
    Pattern(Arc<dyn ColorProvider>),
}

impl From<Color> for Visual {
    fn from(c: Color) -> Self {
        Self::Color(c)
    }
}

#[derive(Clone)]
pub struct Text {
    pub text: String,
    pub font: Option<String>,
    pub varient: Option<String>,
    pub fontsize: f32,

    pub visual: Visual,
}

impl Default for Text {
    fn default() -> Self {
        Self {
            text: String::new(),
            font: None,
            varient: None,
            fontsize: 128.0,

            visual: Color::WHITE.into(),
        }
    }
}

impl Text {
    async fn get_font(&self, fp: &RwLock<FontProvider>) -> Arc<Font> {
        if let Some(font) = self.font.as_deref() {
            let varient = self.varient.as_deref().unwrap_or("regular");
            return {
                let mut provider = fp.write().await;
                provider.varient(font, varient)
            };
        }

        fp.read().await.default_font()
    }
}

#[derive(Clone, PartialEq)]
struct FontFace {
    typeface: String,
    varient: String,
}

impl FontFace {
    pub fn new(typeface: String, varient: String) -> Self {
        Self { typeface, varient }
    }
}

#[derive(Clone)]
pub struct Operation {
    pub bvisual: Visual,
    pub texts: Vec<Text>,
    pub padding: usize,
    pub align: HorizontalAlign,
    pub forceraw: bool,
    pub aspect: Option<f32>,
    pub outline: bool,
    pub glyph_outline: bool,
    pub baseline: bool,
    pub info: bool,
}

impl Default for Operation {
    fn default() -> Self {
        Self {
            bvisual: Color::TRANSPARENT.into(),
            texts: vec![Text::default()],
            padding: 32,
            align: HorizontalAlign::Left,
            forceraw: false,
            aspect: None,
            outline: false,
            glyph_outline: false,
            baseline: false,
            info: false,
        }
    }
}

impl Operation {
    pub async fn make_image(self, fp: &RwLock<FontProvider>) -> Image {
        let mut fonts: Vec<(FontFace, Arc<Font>)> = vec![];

        let settings = LayoutSettings {
            horizontal_align: self.align,
        };

        let mut layout = Layout::new(settings);
        for text in &self.texts {
            let fontface = FontFace::new(
                text.font.clone().unwrap_or_default(),
                text.varient.clone().unwrap_or("regular".into()),
            );

            let index = match fonts
                .iter()
                .enumerate()
                .filter_map(|(index, (vecface, vecfont))| {
                    if vecface == &fontface {
                        Some(index)
                    } else {
                        None
                    }
                })
                .next()
            {
                Some(i) => i,
                None => {
                    fonts.push((fontface, text.get_font(fp).await));

                    fonts.len() - 1
                }
            };

            layout.append(
                &fonts
                    .iter()
                    .map(|(face, font)| font.clone())
                    .collect::<Vec<Arc<Font>>>(),
                StyledText {
                    text: text.text.as_str(),
                    font_size: text.fontsize,
                    font_index: index,
                    user: text.visual.clone(),
                },
            );
        }

        let (horizontal_pad, vertical_pad) = if let Some(ratio) = self.aspect {
            let current_ratio = layout.width() / layout.height();

            if ratio > current_ratio {
                // we're too tall! pad the width.
                let needed_padding = (((layout.height() + self.padding as f32) * ratio)
                    - layout.width())
                .ceil() as usize;

                if needed_padding < self.padding {
                    // the added padding is less than the desired. We can't set
                    // the needed to the desired our we'd overshoot
                    (needed_padding, needed_padding - self.padding)
                } else {
                    (needed_padding, self.padding)
                }
            } else if ratio < current_ratio {
                // we're too wide! pad the height
                let needed_padding = (((layout.width() + self.padding as f32) / ratio)
                    - layout.height())
                .ceil() as usize;

                if needed_padding < self.padding {
                    (needed_padding - self.padding, needed_padding)
                } else {
                    (self.padding, needed_padding)
                }
            } else {
                (self.padding, self.padding)
            }
        } else {
            (self.padding, self.padding)
        };

        let fonts: Vec<Arc<Font>> = fonts.iter().map(|t| t.1.clone()).collect();
        let width = layout.width().ceil() as usize + horizontal_pad;
        let height = layout.height().ceil() as usize + vertical_pad;
        let mut image = match &self.bvisual {
            Visual::Color(c) => Image::with_color(width, height, *c),
            Visual::Pattern(p) => Image::from_provider(width, height, 0, 0, p.as_ref()),
        };

        let off_x = horizontal_pad as isize / 2;
        let off_y = vertical_pad as isize / 2;
        for glyph in layout.glyphs() {
            let x = glyph.x as isize + off_x;
            let y = glyph.y as isize + off_y;

            let glyph = self.glyph(&fonts, glyph, off_x, off_y);
            image.draw_img(glyph, x, y);
        }

        image
    }

    pub fn full_text(&self) -> String {
        let mut ret = String::new();
        for text in &self.texts {
            ret.push_str(&text.text);
        }
        ret
    }

    //todo: pass glyph an offset so we can align the pattern
    fn glyph(
        &self,
        fonts: &[Arc<Font>],
        glyph: GlyphPosition<Visual>,
        off_x: isize,
        off_y: isize,
    ) -> Image {
        let font = &fonts[glyph.font_index];
        let (metrics, raster) = font.rasterize(glyph.c, glyph.font_size);

        match glyph.user {
            Visual::Color(c) => Image::from_buffer(
                metrics.width,
                metrics.height,
                raster,
                Colors::GreyAsAlpha(c),
            ),
            Visual::Pattern(arcpat) => {
                let mut mask = Mask::new(metrics.width, metrics.height);
                let x = glyph.x.ceil() as isize + off_x;
                let y = glyph.y.ceil() as isize + off_y;

                mask.set_from_buf(metrics.width, metrics.height, &raster, 0, 0);

                let mut pattern =
                    Image::from_provider(metrics.width, metrics.height, x, y, arcpat.as_ref());
                pattern.mask(mask, 0, 0);

                pattern
            }
        }
    }

    fn color<S: AsRef<str>>(s: S) -> Option<Color> {
        match s.as_ref() {
            "transparent" => return Some(Color::TRANSPARENT),
            "black" => return Some(Color::BLACK),
            "red" => return Some(Color::RED),
            "green" => return Some(Color::GREEN),
            "blue" => return Some(Color::BLUE),
            "yellow" => return Some(Color::YELLOW),
            "fuchsia" | "magenta" => return Some(Color::FUCHSIA),
            "aqua" | "cyan" => return Some(Color::AQUA),
            "white" => return Some(Color::WHITE),
            _ => (),
        }

        let hexpair = |p: &[char]| -> Option<u8> {
            if let (Some(u), Some(l)) = (p[0].to_digit(16), p[1].to_digit(16)) {
                return Some((u * 16 + l) as u8);
            }
            None
        };

        // Maybe it's a full RGB hex?
        if s.as_ref().len() == 6 {
            let chars: Vec<char> = s.as_ref().chars().collect();
            let mut components: Vec<u8> = chars.chunks(2).filter_map(hexpair).collect();
            components.push(255);

            if let Ok(clr) = Color::try_from(&components[..]) {
                return Some(clr);
            }
        }

        // Full RGBA hex?
        if s.as_ref().len() == 8 {
            let chars: Vec<char> = s.as_ref().chars().collect();
            let components: Vec<u8> = chars.chunks(2).filter_map(hexpair).collect();

            if let Ok(clr) = Color::try_from(&components[..]) {
                return Some(clr);
            }
        }

        // Half RGB hex?
        if s.as_ref().len() == 3 {
            let mut components: Vec<u8> = s
                .as_ref()
                .chars()
                .filter_map(|c| hexpair(&[c, c]))
                .collect();
            components.push(255);

            if let Ok(clr) = Color::try_from(&components[..]) {
                return Some(clr);
            }
        }

        // Half RGBA hex?
        if s.as_ref().len() == 4 {
            let components: Vec<u8> = s
                .as_ref()
                .chars()
                .filter_map(|c| hexpair(&[c, c]))
                .collect();

            if let Ok(clr) = Color::try_from(&components[..]) {
                return Some(clr);
            }
        }

        None
    }

    fn color_or<S: AsRef<str>>(string: Option<S>, color: Color) -> Color {
        if let Some(string) = string {
            Self::color(string).unwrap_or(color)
        } else {
            color
        }
    }

    fn pattern<P: AsRef<str>>(string: P) -> Option<Visual> {
        match string.as_ref() {
            "trans" => Some(Visual::Pattern(Arc::new(Stripes {
                colors: vec![(85, 205, 252).into(), Color::WHITE, (247, 168, 184).into()],
                stripe_width: 16,
                slope: 2.0,
            }))),
            "nonbinary" => Some(Visual::Pattern(Arc::new(Stripes {
                colors: vec![
                    (255, 244, 48).into(),
                    Color::WHITE,
                    (156, 89, 209).into(),
                    Color::BLACK,
                ],
                stripe_width: 16,
                slope: 2.0,
            }))),
            _ => None,
        }
    }

    fn push_parameter(&mut self, parameter: Parameter) {
        match parameter {
            Parameter::Bool(name) => self.parse_bool(name),
            Parameter::Value(key, value) => self.parse_value(key, value),
        }
    }

    fn parse_bool<S: AsRef<str>>(&mut self, name: S) {
        match name.as_ref() {
            "forceraw" => self.forceraw = true,
            _ => (),
        }
    }

    fn parse_value(&mut self, key: String, value: String) {
        let current = self.texts.last_mut().unwrap();

        match key.as_str() {
            "text" => {
                if current.text.is_empty() {
                    self.texts[0].text = value;
                    return;
                } else {
                    let mut next = current.clone();
                    next.text = value;

                    self.texts.push(next);
                }
            }
            "font" => current.font = Some(value),
            "varient" => current.varient = Some(value),
            "fs" | "fontsize" => current.fontsize = value.parse().unwrap_or(128.0),
            "c" | "color" => {
                current.visual = Visual::Color(Self::color_or(Some(value), Color::WHITE))
            }
            "pattern" => {
                if let Some(pat) = Self::pattern(value) {
                    current.visual = pat;
                }
            }

            "align" => match value.as_str() {
                "center" => self.align = HorizontalAlign::Center,
                "right" => self.align = HorizontalAlign::Right,
                _ => self.align = HorizontalAlign::Left,
            },
            "aspect" => self.aspect = value.parse().ok(),
            "bc" | "bcolor" => {
                self.bvisual = Visual::Color(Self::color_or(Some(value), Color::WHITE))
            }
            "bpattern" => {
                if let Some(pat) = Self::pattern(value) {
                    self.bvisual = pat;
                }
            }
            _ => (),
        }
    }
}

impl From<Query> for Operation {
    fn from(query: Query) -> Self {
        let mut ret = Self::default();

        for param in query.into_iter() {
            ret.push_parameter(param);
        }

        ret
    }
}
