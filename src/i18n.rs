mod en;
mod pt_br;
mod strings;

pub use strings::*;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    En,
    #[serde(rename = "pt-BR")]
    PtBr,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::En, Self::PtBr];

    pub fn strings(self) -> &'static Strings {
        match self {
            Self::En => &en::STRINGS,
            Self::PtBr => &pt_br::STRINGS,
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::PtBr => "pt-BR",
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::PtBr => "Português",
        }
    }

    pub fn from_locale(locale: &str) -> Option<Self> {
        let primary = locale
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match primary.as_str() {
            "en" => Some(Self::En),
            "pt" => Some(Self::PtBr),
            _ => None,
        }
    }

    pub fn from_system() -> Self {
        sys_locale::get_locales()
            .find_map(|locale| Self::from_locale(&locale))
            .unwrap_or_default()
    }
}

impl std::str::FromStr for Language {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::from_locale(input.trim()).ok_or_else(|| {
            let known: Vec<_> = Self::ALL.iter().map(|language| language.tag()).collect();
            format!("{input:?}: {}", known.join(", "))
        })
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(language: Language) {
    CURRENT.store(language as u8, Ordering::Relaxed);
}

pub fn current() -> Language {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Language::PtBr,
        _ => Language::En,
    }
}

pub fn text() -> &'static Strings {
    current().strings()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_map_to_supported_languages_and_fall_back_to_english() {
        assert_eq!(Language::from_locale("pt-BR"), Some(Language::PtBr));
        assert_eq!(Language::from_locale("pt_PT.UTF-8"), Some(Language::PtBr));
        assert_eq!(Language::from_locale("en_US.UTF-8"), Some(Language::En));
        assert_eq!(Language::from_locale("EN"), Some(Language::En));
        assert_eq!(Language::from_locale("de-DE"), None);
        assert_eq!(Language::from_locale(""), None);
        assert_eq!("pt-br".parse::<Language>(), Ok(Language::PtBr));
        assert!("fr".parse::<Language>().is_err());
    }

    #[test]
    fn languages_serialize_with_their_tags() {
        for language in Language::ALL {
            let json = serde_json::to_string(&language).unwrap();
            assert_eq!(json, format!("\"{}\"", language.tag()));
            assert_eq!(serde_json::from_str::<Language>(&json).unwrap(), language);
            assert_eq!(Language::from_locale(language.tag()), Some(language));
        }
    }

    #[test]
    fn every_language_shows_room_codes_that_parse_in_any_language() {
        use crate::network::relay::Code;
        for language in Language::ALL {
            let t = language.strings();
            let example = Code::parse(t.common.example_code).expect("example code parses");
            assert!(t.home.code_tooltip.contains(t.common.example_code));
            assert!(t.errors.code_hint.contains(t.common.example_code));
            for other in Language::ALL {
                assert_eq!(
                    Code::parse(other.strings().common.example_code),
                    Some(example.clone())
                );
            }
        }
    }
}
