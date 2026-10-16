//! Translations. English text is the key (gettext style), so untranslated
//! strings fall back to English automatically.
//!
//! Built-in languages are embedded from `lang/*.json`. Extra or improved
//! translations can be dropped into a `lang` folder next to the exe as
//! `<code>.json`, without recompiling.

use std::collections::HashMap;
use std::sync::RwLock;

pub struct Language {
    pub code: String,
    pub name: String,
    data: HashMap<String, String>,
}

const BUILTIN: &[(&str, &str)] = &[
    ("es", include_str!("../lang/es.json")),
    ("de", include_str!("../lang/de.json")),
    ("fr", include_str!("../lang/fr.json")),
    ("pt-BR", include_str!("../lang/pt-BR.json")),
    ("it", include_str!("../lang/it.json")),
    ("ru", include_str!("../lang/ru.json")),
    ("zh-CN", include_str!("../lang/zh-CN.json")),
    ("ja", include_str!("../lang/ja.json")),
    ("ko", include_str!("../lang/ko.json")),
    ("pl", include_str!("../lang/pl.json")),
];

static CURRENT: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);
static CURRENT_CODE: RwLock<String> = RwLock::new(String::new());

fn parse(code: &str, json: &str) -> Option<Language> {
    let mut data: HashMap<String, String> = crate::json::compat::from_str(json).ok()?;
    let name = data.remove("_language").unwrap_or_else(|| code.to_string());
    data.retain(|k, v| !k.starts_with('_') && !v.trim().is_empty());
    Some(Language {
        code: code.to_string(),
        name,
        data,
    })
}

/// All available languages: English, built-ins, then any external files
/// (which override a built-in with the same code).
pub fn languages() -> Vec<Language> {
    let mut out = vec![Language {
        code: "en".into(),
        name: "English".into(),
        data: HashMap::new(),
    }];
    for (code, json) in BUILTIN {
        if let Some(l) = parse(code, json) {
            out.push(l);
        }
    }
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("lang")));
    if let Some(dir) = dir {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") {
                let code = p
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if let Some(l) = std::fs::read_to_string(&p)
                    .ok()
                    .and_then(|s| parse(&code, &s))
                {
                    out.retain(|x| x.code != l.code);
                    out.push(l);
                }
            }
        }
    }
    out
}

/// Choose the best language for a Windows locale name like `de-DE`.
pub fn best_match(locale: &str, available: &[Language]) -> String {
    let lower = locale.to_lowercase();
    if let Some(l) = available.iter().find(|l| l.code.to_lowercase() == lower) {
        return l.code.clone();
    }
    let primary = lower.split(['-', '_']).next().unwrap_or("");
    // Any Portuguese → pt-BR, any Chinese → zh-CN until more variants exist.
    if let Some(l) = available
        .iter()
        .find(|l| l.code.to_lowercase().split('-').next() == Some(primary))
    {
        return l.code.clone();
    }
    "en".into()
}

/// Activate a language ("" or "system" = follow Windows).
pub fn set_language(code: &str) {
    let langs = languages();
    let code = if code.is_empty() || code == "system" {
        best_match(&system_locale(), &langs)
    } else {
        code.to_string()
    };
    let data = langs.into_iter().find(|l| l.code == code).map(|l| l.data);
    *CURRENT.write().unwrap() = data.filter(|d| !d.is_empty());
    *CURRENT_CODE.write().unwrap() = code;
}

pub fn current_code() -> String {
    CURRENT_CODE.read().unwrap().clone()
}

/// Translate a string.
pub fn t(s: &str) -> String {
    match CURRENT.read().unwrap().as_ref().and_then(|m| m.get(s)) {
        Some(v) => v.clone(),
        None => s.to_string(),
    }
}

/// Translate and fill `{name}` placeholders.
pub fn tf(s: &str, args: &[(&str, String)]) -> String {
    let mut out = t(s);
    for (k, v) in args {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

/// The user's Windows display language, e.g. `en-US`.
pub fn system_locale() -> String {
    #[cfg(windows)]
    {
        crate::platform::windows::user_locale()
    }
    #[cfg(not(windows))]
    {
        std::env::var("LANG")
            .unwrap_or_else(|_| "en-US".into())
            .split('.')
            .next()
            .unwrap_or("en")
            .replace('_', "-")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_load_and_translate() {
        let langs = languages();
        assert!(langs.len() >= 9);
        for l in &langs[1..] {
            assert!(!l.data.is_empty(), "{} is empty", l.code);
            // Placeholders must survive translation.
            for (k, v) in &l.data {
                for ph in [
                    "{n}", "{name}", "{size}", "{count}", "{path}", "{kb}", "{error}", "{done}",
                    "{total}",
                ] {
                    if k.contains(ph) {
                        assert!(v.contains(ph), "{}: '{k}' -> '{v}' lost {ph}", l.code);
                    }
                }
            }
        }
        assert_eq!(best_match("de-AT", &langs), "de");
        assert_eq!(best_match("pt-PT", &langs), "pt-BR");
        assert_eq!(best_match("zh-TW", &langs), "zh-CN");
        assert_eq!(best_match("ko-KR", &langs), "ko");
        assert_eq!(best_match("pl-PL", &langs), "pl");
        assert_eq!(best_match("nl-NL", &langs), "en");
        set_language("de");
        assert_eq!(t("Uninstall"), "Deinstallieren");
        assert_eq!(t("No such string"), "No such string");
        set_language("en");
        assert_eq!(tf("{n} items", &[("n", "3".into())]), "3 items");
    }
}
