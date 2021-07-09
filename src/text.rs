use std::convert::TryFrom;

use fontster::{Color, Settings};
use thiserror::Error;

use crate::query::Query;

#[derive(Clone)]
pub struct Text {
    pub text: String,
    color: Option<String>,
    bcolor: Option<String>,
    pub italic: bool,
}

impl Text {
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
            let mut components: Vec<u8> = s
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
}

impl TryFrom<Query> for Text {
    type Error = TextError;

    fn try_from(query: Query) -> Result<Self, Self::Error> {
        Ok(Self {
            text: query.get_first_value("text").ok_or(TextError::NoText)?,
            color: query.get_first_value("color"),
            bcolor: query.get_first_value("bcolor"),
            italic: query.bool_present("italicize"),
        })
    }
}

impl Into<Settings> for Text {
    fn into(self) -> Settings {
        Settings {
            text_color: self
                .color
                .map(|s| Self::color(s).unwrap_or(Color::WHITE))
                .unwrap_or(Color::WHITE),
            background_color: self
                .bcolor
                .map(|s| Self::color(s).unwrap_or(Color::TRANSPARENT))
                .unwrap_or(Color::TRANSPARENT),
            draw_baseline: false,
            draw_glyph_outline: false,
            draw_sentence_outline: false,
        }
    }
}

#[derive(Error, Debug)]
pub enum TextError {
    #[error("Text to rasterize must be provided")]
    NoText,
}
