//! Plain-language search: "find and wipe everything related to that video
//! converter I downloaded last week" becomes a structured query (action,
//! time window, size, topics and keywords) that is matched against installed
//! programs and install logs. Runs locally with simple rules; nothing is sent
//! anywhere, and removal always goes through the usual review.

use crate::time::Date as NaiveDate;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Find,
    Remove,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub action: Action,
    /// Installed on or after / before these dates.
    pub since: Option<NaiveDate>,
    pub until: Option<NaiveDate>,
    /// At least this many bytes.
    pub min_size: Option<u64>,
    /// Not opened for at least this many days.
    pub unused_days: Option<u32>,
    /// Topic names recognized ("video converter", "browser"…).
    pub topics: Vec<&'static str>,
    /// Words a match should contain (any of them).
    pub keywords: Vec<String>,
    /// Limit to the N biggest ("top 5 …").
    pub top: Option<usize>,
}

/// Topics and the words that identify programs about them.
const TOPICS: &[(&str, &[&str], &[&str])] = &[
    // (topic, trigger words in the question, words to look for in programs)
    (
        "video converter",
        &[
            "video converter",
            "video convert",
            "converter",
            "transcod",
            "encoder",
        ],
        &[
            "convert",
            "video",
            "ffmpeg",
            "handbrake",
            "encod",
            "transcod",
            "format factory",
            "any video",
            "freemake",
            "movavi",
            "wondershare",
        ],
    ),
    (
        "video player",
        &["video player", "media player", "movie player"],
        &[
            "player",
            "vlc",
            "media",
            "mpc",
            "potplayer",
            "kmplayer",
            "gom",
        ],
    ),
    (
        "music",
        &["music", "audio player", "mp3"],
        &[
            "music", "audio", "mp3", "spotify", "itunes", "winamp", "foobar", "aimp",
        ],
    ),
    (
        "browser",
        &["browser", "web browser"],
        &[
            "browser", "chrome", "firefox", "edge", "opera", "brave", "vivaldi", "yandex",
            "chromium",
        ],
    ),
    (
        "game",
        &["game", "games", "gaming", "launcher"],
        &[
            "game",
            "steam",
            "epic",
            "ubisoft",
            "ea app",
            "origin",
            "battle.net",
            "gog",
            "riot",
            "launcher",
        ],
    ),
    (
        "antivirus",
        &["antivirus", "anti-virus", "security suite", "virus"],
        &[
            "antivirus",
            "security",
            "mcafee",
            "norton",
            "avast",
            "avg",
            "kaspersky",
            "eset",
            "bitdefender",
            "avira",
            "trend micro",
            "malwarebytes",
        ],
    ),
    (
        "toolbar",
        &["toolbar", "adware", "search bar"],
        &["toolbar", "search", "bar", "ask", "conduit", "babylon"],
    ),
    (
        "cleaner",
        &["cleaner", "optimizer", "booster", "speed up", "tune up"],
        &["clean", "optimi", "boost", "speed", "tune", "care", "fix"],
    ),
    (
        "pdf",
        &["pdf"],
        &["pdf", "acrobat", "reader", "foxit", "sumatra", "nitro"],
    ),
    (
        "photo editor",
        &["photo", "image editor", "picture"],
        &[
            "photo",
            "image",
            "picture",
            "paint",
            "gimp",
            "photoshop",
            "lightroom",
            "editor",
        ],
    ),
    (
        "office",
        &["office", "word processor", "spreadsheet"],
        &[
            "office",
            "word",
            "excel",
            "libreoffice",
            "openoffice",
            "wps",
        ],
    ),
    (
        "archiver",
        &["zip", "archiver", "unzip", "rar", "7-zip", "compression"],
        &["zip", "rar", "7-zip", "archive", "peazip", "bandizip"],
    ),
    (
        "driver",
        &["driver", "drivers"],
        &["driver", "nvidia", "amd", "intel", "realtek", "chipset"],
    ),
    (
        "chat",
        &["chat", "messenger", "messaging", "video call"],
        &[
            "chat",
            "messenger",
            "discord",
            "skype",
            "zoom",
            "teams",
            "slack",
            "telegram",
            "whatsapp",
        ],
    ),
    (
        "vpn",
        &["vpn", "proxy"],
        &[
            "vpn",
            "proxy",
            "tunnel",
            "nord",
            "express",
            "surfshark",
            "proton",
        ],
    ),
    (
        "downloader",
        &["downloader", "download manager", "torrent"],
        &[
            "download",
            "torrent",
            "idm",
            "jdownloader",
            "utorrent",
            "bittorrent",
            "qbittorrent",
        ],
    ),
    (
        "trial",
        &[
            "trial",
            "trialware",
            "bloatware",
            "crapware",
            "preinstalled",
            "pre-installed",
        ],
        &[
            "trial",
            "offer",
            "mcafee",
            "norton",
            "wildtangent",
            "booking",
            "dropbox promotion",
            "expressvpn",
        ],
    ),
];

const STOP: &[&str] = &[
    "a",
    "an",
    "the",
    "that",
    "this",
    "those",
    "these",
    "i",
    "me",
    "my",
    "we",
    "you",
    "it",
    "its",
    "and",
    "or",
    "of",
    "for",
    "to",
    "in",
    "on",
    "at",
    "from",
    "with",
    "all",
    "everything",
    "anything",
    "something",
    "related",
    "relating",
    "about",
    "program",
    "programs",
    "app",
    "apps",
    "application",
    "applications",
    "software",
    "stuff",
    "thing",
    "things",
    "find",
    "show",
    "list",
    "search",
    "look",
    "which",
    "what",
    "wipe",
    "remove",
    "uninstall",
    "delete",
    "erase",
    "get",
    "rid",
    "clean",
    "kill",
    "nuke",
    "purge",
    "please",
    "can",
    "could",
    "would",
    "downloaded",
    "installed",
    "install",
    "download",
    "got",
    "put",
    "added",
    "last",
    "past",
    "previous",
    "ago",
    "since",
    "before",
    "after",
    "week",
    "weeks",
    "month",
    "months",
    "day",
    "days",
    "year",
    "years",
    "today",
    "yesterday",
    "recently",
    "recent",
    "new",
    "newest",
    "latest",
    "old",
    "biggest",
    "largest",
    "big",
    "large",
    "huge",
    "top",
    "most",
    "space",
    "wasters",
    "waster",
    "haven't",
    "havent",
    "have",
    "not",
    "never",
    "opened",
    "used",
    "use",
    "open",
    "unused",
    "than",
    "bigger",
    "larger",
    "over",
    "more",
    "gb",
    "mb",
    "it's",
    "space-wasters",
    "space-waster",
    "is",
    "are",
    "was",
    "were",
    "be",
    "been",
    "there",
    "some",
    "any",
    "only",
    "just",
    "also",
    "like",
    "kind",
    "sort",
    "one",
];

fn number_word(w: &str) -> Option<i64> {
    match w {
        "a" | "an" | "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        "twelve" => Some(12),
        _ => w.parse().ok(),
    }
}

fn unit_days(w: &str) -> Option<i64> {
    let w = w.trim_end_matches('s');
    match w {
        "day" => Some(1),
        "week" => Some(7),
        "month" => Some(30),
        "year" => Some(365),
        _ => None,
    }
}

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// Understand a question asked on `today`.
pub fn parse(text: &str, today: NaiveDate) -> Query {
    let lower = text.to_lowercase();
    let words: Vec<String> = lower
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-' || c == '.' || c == '+'))
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();
    let has = |w: &str| words.iter().any(|x| x == w);
    let phrase = |p: &str| lower.contains(p);

    let remove = [
        "wipe",
        "remove",
        "uninstall",
        "delete",
        "erase",
        "nuke",
        "purge",
        "kill",
    ]
    .iter()
    .any(|w| has(w))
        || phrase("get rid of")
        || phrase("clean up") && !phrase("cleaner");
    let mut q = Query {
        action: if remove { Action::Remove } else { Action::Find },
        since: None,
        until: None,
        min_size: None,
        unused_days: None,
        topics: Vec::new(),
        keywords: Vec::new(),
        top: None,
    };

    // "haven't opened in 6 months" / "not used for a year" / "unused".
    let unused = phrase("haven't opened")
        || phrase("havent opened")
        || phrase("haven't used")
        || phrase("havent used")
        || phrase("not opened")
        || phrase("not used")
        || phrase("never used")
        || phrase("never opened")
        || has("unused");

    // Time windows: "last week", "past 3 days", "2 months ago", "yesterday"…
    let mut window: Option<i64> = None;
    for i in 0..words.len() {
        let w = words[i].as_str();
        if let Some(d) = unit_days(w) {
            let prev = i.checked_sub(1).map(|j| words[j].as_str());
            let n = prev.and_then(number_word);
            let qualifier = prev.filter(|p| ["last", "past", "previous", "this"].contains(p));
            if let Some(n) = n {
                window = Some(n * d);
            } else if qualifier.is_some() {
                window = Some(d);
            }
            // "6 months" after "in"/"for" belongs to the unused clause.
            if unused && window.is_some() {
                q.unused_days = window.map(|d| d as u32);
                window = None;
            }
        }
    }
    if has("today") {
        q.since = Some(today);
    } else if has("yesterday") {
        q.since = Some(today.add_days(-1));
        q.until = Some(today.add_days(-1));
    } else if has("recently") || has("recent") || has("new") || has("newest") || has("latest") {
        window = window.or(Some(14));
    }
    if let Some(d) = window {
        // "2 weeks ago" means around then, "last week" means since then.
        let ago = has("ago");
        q.since = Some(today.add_days(-(d + if ago { 4 } else { 0 })));
        if ago {
            q.until = Some(today.add_days(-((d - 4).max(0))));
        }
    }
    for (i, m) in MONTHS.iter().enumerate() {
        if has(m) || (m.len() > 3 && has(&m[..3]) && *m != "may") {
            let month = i as u32 + 1;
            let year = if month > today.month() {
                today.year() - 1
            } else {
                today.year()
            };
            if let Some(first) = NaiveDate::from_ymd(year, month, 1) {
                let next = if month == 12 {
                    NaiveDate::from_ymd(year + 1, 1, 1)
                } else {
                    NaiveDate::from_ymd(year, month + 1, 1)
                };
                q.since = Some(first);
                q.until = next.map(|n| n.add_days(-1));
            }
        }
    }
    if unused && q.unused_days.is_none() {
        q.unused_days = Some(180);
    }

    // Size: "bigger than 2 GB", "over 500 mb", "huge".
    for i in 0..words.len() {
        let unit = match words[i].as_str() {
            "gb" | "gigabytes" | "gig" | "gigs" => Some(1u64 << 30),
            "mb" | "megabytes" | "megs" => Some(1u64 << 20),
            _ => None,
        };
        if let (Some(u), Some(n)) = (
            unit,
            i.checked_sub(1).and_then(|j| words[j].parse::<f64>().ok()),
        ) {
            q.min_size = Some((n * u as f64) as u64);
        } else if let Some(stripped) = words[i]
            .strip_suffix("gb")
            .or_else(|| words[i].strip_suffix("mb"))
        {
            if let Ok(n) = stripped.parse::<f64>() {
                let u = if words[i].ends_with("gb") {
                    1u64 << 30
                } else {
                    1u64 << 20
                };
                q.min_size = Some((n * u as f64) as u64);
            }
        }
    }
    if q.min_size.is_none()
        && (has("huge")
            || has("biggest")
            || has("largest")
            || phrase("space wasters")
            || phrase("space-wasters")
            || phrase("space hog"))
    {
        q.min_size = Some(200 << 20);
    }
    // "top 5".
    if let Some(i) = words.iter().position(|w| w == "top") {
        q.top = words
            .get(i + 1)
            .and_then(|w| number_word(w))
            .map(|n| n as usize);
    }

    // Topics, then whatever words are left.
    let mut covered: Vec<&str> = Vec::new();
    for (topic, triggers, _) in TOPICS {
        if let Some(tr) = triggers.iter().find(|t| phrase(t)) {
            q.topics.push(topic);
            covered.extend(tr.split(' '));
        }
    }
    for w in &words {
        let plain = w.trim_matches('\'');
        if plain.len() < 2
            || STOP.contains(&plain)
            || number_word(plain).is_some()
            || MONTHS.contains(&plain)
        {
            continue;
        }
        if covered
            .iter()
            .any(|c| plain.starts_with(c) || c.starts_with(plain))
        {
            continue;
        }
        if !q.keywords.iter().any(|k| k == plain) {
            q.keywords.push(plain.to_string());
        }
    }
    q
}

impl Query {
    /// Words to look for: the keywords plus each topic's vocabulary.
    pub fn terms(&self) -> Vec<String> {
        let mut v = self.keywords.clone();
        for (topic, _, words) in TOPICS {
            if self.topics.contains(topic) {
                v.extend(words.iter().map(|w| w.to_string()));
            }
        }
        v
    }

    /// How well a text (name, publisher, folder) matches: 0 = not at all.
    pub fn score_text(&self, text: &str) -> u32 {
        let l = text.to_lowercase();
        let mut s = 0;
        for k in &self.keywords {
            if l.contains(k.as_str()) {
                s += 3;
            }
        }
        for (topic, _, words) in TOPICS {
            if self.topics.contains(topic) && words.iter().any(|w| l.contains(w)) {
                s += 2;
            }
        }
        s
    }

    /// Whether the query names anything to match by text.
    pub fn has_terms(&self) -> bool {
        !self.keywords.is_empty() || !self.topics.is_empty()
    }

    pub fn date_ok(&self, date: Option<NaiveDate>) -> bool {
        match date {
            Some(d) => self.since.is_none_or(|s| d >= s) && self.until.is_none_or(|u| d <= u),
            None => self.since.is_none() && self.until.is_none(),
        }
    }

    /// A short description of how the question was understood.
    pub fn explain(&self) -> String {
        let mut parts = vec![if self.action == Action::Remove {
            "Remove".to_string()
        } else {
            "Find".to_string()
        }];
        if let Some(n) = self.top {
            parts.push(format!("the top {n}"));
        }
        parts.push("programs".into());
        if !self.topics.is_empty() {
            parts.push(format!("about {}", self.topics.join(", ")));
        }
        if !self.keywords.is_empty() {
            parts.push(format!("matching \"{}\"", self.keywords.join("\", \"")));
        }
        match (self.since, self.until) {
            (Some(s), Some(u)) if s == u => parts.push(format!("installed on {s}")),
            (Some(s), Some(u)) => parts.push(format!("installed between {s} and {u}")),
            (Some(s), None) => parts.push(format!("installed since {s}")),
            _ => {}
        }
        if let Some(b) = self.min_size {
            parts.push(format!("using at least {}", crate::model::human_size(b)));
        }
        if let Some(d) = self.unused_days {
            parts.push(format!("not opened for {d} days or more"));
        }
        parts.join(" ")
    }
}

/// "2026-10-08" (or "20261008") → a date.
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    NaiveDate::parse_ymd(s).or_else(|| NaiveDate::parse_compact(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_ymd(s).unwrap()
    }

    #[test]
    fn the_video_converter_example() {
        let q = parse(
            "Find and wipe everything related to that video converter I downloaded last week",
            d("2026-10-15"),
        );
        assert_eq!(q.action, Action::Remove);
        assert_eq!(q.since, Some(d("2026-10-08")));
        assert_eq!(q.topics, vec!["video converter"]);
        assert!(q.keywords.is_empty(), "{:?}", q.keywords);
        assert!(q.score_text("VidConvert Pro 5") > 0);
        assert!(q.score_text("HandBrake") > 0);
        assert_eq!(q.score_text("Notepad++"), 0);
    }

    #[test]
    fn space_wasters_not_opened() {
        let q = parse(
            "What are the top 5 space-wasters I haven't opened in 6 months?",
            d("2026-10-15"),
        );
        assert_eq!(q.action, Action::Find);
        assert_eq!(q.top, Some(5));
        assert_eq!(q.unused_days, Some(180));
        assert!(q.min_size.is_some());
        assert!(q.since.is_none());
        assert!(!q.has_terms(), "{q:?}");
    }

    #[test]
    fn names_dates_and_sizes() {
        let q = parse("uninstall spotify", d("2026-10-15"));
        assert_eq!(
            (q.action, q.keywords.clone()),
            (Action::Remove, vec!["spotify".to_string()])
        );
        let q = parse(
            "show browsers installed 2 weeks ago bigger than 1.5 GB",
            d("2026-10-15"),
        );
        assert_eq!(q.topics, vec!["browser"]);
        assert_eq!(q.since, Some(d("2026-09-27")));
        assert_eq!(q.until, Some(d("2026-10-05")));
        assert_eq!(q.min_size, Some((1.5 * (1u64 << 30) as f64) as u64));
        let q = parse("games from august", d("2026-10-15"));
        assert_eq!(
            (q.since, q.until),
            (Some(d("2026-08-01")), Some(d("2026-08-31")))
        );
        let q = parse("what did I install yesterday", d("2026-10-15"));
        assert_eq!(
            (q.since, q.until),
            (Some(d("2026-10-14")), Some(d("2026-10-14")))
        );
        assert!(q.date_ok(Some(d("2026-10-14"))));
        assert!(!q.date_ok(Some(d("2026-10-13"))));
    }
}
