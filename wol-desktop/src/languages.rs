use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguageInfo {
    pub symbol: String,
    pub lib: String,
    pub locale: String,
    pub name: String,
    pub region: u32,
    #[serde(rename = "mepsId")]
    pub meps_id: Option<u32>,
    pub home: String,
}

impl LanguageInfo {
    /// Build a full `https://` home URL for the given base host.
    pub fn home_url(&self, base_host: &str) -> String {
        if self.home.starts_with('/') {
            format!("https://{base_host}{}", self.home)
        } else {
            self.home.clone()
        }
    }
}

static LANGUAGES: OnceLock<Vec<LanguageInfo>> = OnceLock::new();

pub fn all_languages() -> &'static [LanguageInfo] {
    LANGUAGES.get_or_init(|| {
        let raw = include_str!("languages.json");
        serde_json::from_str(raw).unwrap_or_else(|_| Vec::new())
    })
}

pub fn find_by_symbol(symbol: &str) -> Option<&'static LanguageInfo> {
    let sym = symbol.trim();
    all_languages().iter().find(|l| {
        l.symbol.eq_ignore_ascii_case(sym) || l.lib.eq_ignore_ascii_case(sym)
    })
}

pub fn find_by_locale(locale: &str) -> Option<&'static LanguageInfo> {
    let loc = locale.trim();
    all_languages().iter().find(|l| l.locale.eq_ignore_ascii_case(loc))
}

pub fn default_language() -> &'static LanguageInfo {
    find_by_locale("es").unwrap_or_else(|| {
        all_languages()
            .first()
            .expect("languages table must not be empty")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_scraped_languages_table() {
        let langs = all_languages();
        assert!(langs.len() >= 900, "expected at least 900 languages, got {}", langs.len());

        let es = find_by_symbol("S").expect("Spanish must exist");
        assert_eq!(es.locale, "es");
        assert_eq!(es.region, 4);
        assert_eq!(es.lib, "s");
        assert_eq!(es.meps_id, Some(1));
        assert_eq!(es.home, "/es/wol/h/r4/lp-s");
        assert_eq!(es.home_url("wol.example.org"), "https://wol.example.org/es/wol/h/r4/lp-s");

        let en = find_by_symbol("E").expect("English must exist");
        assert_eq!(en.locale, "en");
        assert_eq!(en.region, 1);
        assert_eq!(en.lib, "e");
        assert_eq!(en.meps_id, Some(0));
        assert_eq!(en.home, "/en/wol/h/r1/lp-e");
    }

    #[test]
    fn lookup_by_locale() {
        let de = find_by_locale("de").expect("German must exist");
        assert_eq!(de.symbol, "X");
        assert_eq!(de.region, 10);
        assert_eq!(de.lib, "x");
    }
}
