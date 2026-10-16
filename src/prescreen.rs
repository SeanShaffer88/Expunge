//! Pre-install screening: look inside a setup file before running it for
//! bundled adware ("offers"), bundler SDKs, coin miners and other warning
//! signs, and say where it was downloaded from and who signed it.
//!
//! This is pattern matching on the file's contents, done locally. It can't
//! see inside compressed payloads or what a web installer downloads later,
//! and says so; it is a warning system, not an antivirus.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Info,
    Caution,
    Danger,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Finding {
    pub level: Level,
    pub title: String,
    pub detail: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Screening {
    pub findings: Vec<Finding>,
    /// Where the file came from (Mark of the Web), when Windows recorded it.
    pub source: Option<String>,
    pub signer: Option<String>,
    pub installer: String,
}

impl Screening {
    pub fn worst(&self) -> Level {
        self.findings
            .iter()
            .map(|f| f.level)
            .max()
            .unwrap_or(Level::Info)
    }
}

/// Bundler and "offer" SDKs used to push extra software during setup.
const BUNDLERS: &[(&str, &str)] = &[
    ("opencandy", "OpenCandy"),
    ("installcore", "InstallCore"),
    ("icloader", "InstallCore"),
    ("amonetize", "Amonetize"),
    ("downloadadmin", "DownloadAdmin"),
    ("somoto", "Somoto"),
    ("linkury", "Linkury"),
    ("crossrider", "Crossrider"),
    ("conduit", "Conduit toolbar"),
    ("babylon toolbar", "Babylon toolbar"),
    ("ask toolbar", "Ask toolbar"),
    ("apnmedia", "Ask/APN toolbar"),
    ("delta-search", "Delta Search"),
    ("wajam", "Wajam"),
    ("superfish", "Superfish"),
    ("mypcbackup", "MyPCBackup"),
    ("bytefence", "ByteFence"),
    ("reimageplus", "Reimage"),
    ("slimcleaner", "SlimCleaner"),
    ("pcspeedup", "PC speed-up offer"),
    ("relevantknowledge", "RelevantKnowledge"),
    ("solimba", "Solimba"),
    ("outbrowse", "OutBrowse"),
    ("firseria", "Firseria"),
    ("installmonetizer", "InstallMonetizer"),
    ("airinstaller", "AirInstaller"),
    ("softonic", "Softonic downloader"),
    ("installiq", "InstallIQ"),
    ("prismdeploy", "Prism offers"),
    ("offerscreen", "offer screens"),
];

/// Coin miners have no place in an installer.
const MINERS: &[&str] = &[
    "stratum+tcp://",
    "xmrig",
    "coinhive",
    "cryptonight",
    "nicehash",
    "minergate",
];

/// Phrases offer screens use.
const OFFER_PHRASES: &[&str] = &[
    "special offer",
    "sponsored offer",
    "partner offer",
    "recommended offer",
    "install the following",
    "set as my default search",
    "make it my homepage",
    "express installation (recommended)",
];

/// Antivirus and "optimizer" trials pushed by partner deals.
const TRIAL_OFFERS: &[(&str, &str)] = &[
    ("mcafee", "McAfee"),
    ("norton", "Norton"),
    ("avast_free_antivirus", "Avast"),
    ("avg_antivirus", "AVG"),
    ("opera_installer", "Opera browser"),
    ("operasetup", "Opera browser"),
    ("chromesetup", "Google Chrome"),
    ("yandex", "Yandex"),
    ("driver booster", "Driver Booster"),
    ("advanced systemcare", "Advanced SystemCare"),
    ("ccleaner", "CCleaner"),
];

fn lower_ascii(b: &[u8]) -> Vec<u8> {
    b.iter().map(|c| c.to_ascii_lowercase()).collect()
}

fn find(hay: &[u8], needle: &str) -> bool {
    let n = needle.as_bytes();
    if crate::util::find_bytes(hay, n).is_some() {
        return true;
    }
    // UTF-16LE, as Windows resources store text.
    let wide: Vec<u8> = n.iter().flat_map(|&c| [c, 0]).collect();
    crate::util::find_bytes(hay, &wide).is_some()
}

/// Scan the bytes of a setup file (already lower-cased).
pub fn scan_bytes(hay: &[u8], size: u64) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (needle, name) in BUNDLERS {
        if find(hay, needle) && seen.insert(*name) {
            out.push(Finding {
                level: Level::Danger,
                title: crate::i18n::tf("Bundled adware: {name}", &[("name", name.to_string())]),
                detail: crate::i18n::tf("The setup file contains {name}, software used to install extra programs, toolbars or browser changes. Choose Custom or Advanced installation and untick every offer, or get the program from its official website.", &[("name", name.to_string())]),
            });
        }
    }
    if let Some(m) = MINERS.iter().find(|m| find(hay, m)) {
        out.push(Finding {
            level: Level::Danger,
            title: crate::i18n::t("Coin miner"),
            detail: crate::i18n::tf("The setup file mentions {name}, which is used to mine cryptocurrency with your PC's power. Don't run it unless you know why it is there.", &[("name", m.to_string())]),
        });
    }
    let offers: Vec<&str> = TRIAL_OFFERS
        .iter()
        .filter(|(n, _)| find(hay, n))
        .map(|(_, name)| *name)
        .collect();
    if !offers.is_empty() {
        out.push(Finding {
            level: Level::Caution,
            title: crate::i18n::t("Other software may be offered"),
            detail: crate::i18n::tf("The setup file mentions {list}. Installers often offer such programs on a separate page that is ticked by default. Read every page and untick what you don't want.", &[("list", offers.join(", "))]),
        });
    }
    if let Some(p) = OFFER_PHRASES.iter().find(|p| find(hay, p)) {
        out.push(Finding {
            level: Level::Caution,
            title: crate::i18n::t("Offer screen"),
            detail: crate::i18n::tf("The setup contains text used by offer screens (\"{text}\"). Look for pre-ticked boxes and decline offers you don't want.", &[("text", p.to_string())]),
        });
    }
    let web = (find(hay, "http://") || find(hay, "https://"))
        && (find(hay, "download") || find(hay, "inetc") || find(hay, "nsisdl"));
    if size < 6 * 1024 * 1024 && web {
        out.push(Finding {
            level: Level::Info,
            title: crate::i18n::t("Downloads during setup"),
            detail: crate::i18n::t("This looks like a small web installer that downloads the program while it runs, so this check can't see everything it installs. Testing it in Windows Sandbox first shows what it really does."),
        });
    }
    out
}

/// Where Windows says the file was downloaded from (Mark of the Web).
pub fn download_source(path: &Path) -> Option<String> {
    let ads = format!("{}:Zone.Identifier", path.display());
    let text = std::fs::read_to_string(ads).ok()?;
    let get = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string()))
            .filter(|v| !v.is_empty())
    };
    get("HostUrl=").or_else(|| get("ReferrerUrl="))
}

/// Screen a setup file. Reads up to 96 MB of it.
pub fn screen(path: &Path) -> Screening {
    let mut s = Screening {
        installer: crate::pafbuild::detect_installer(path).label().to_string(),
        ..Default::default()
    };
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut buf = Vec::new();
    if let Ok(f) = std::fs::File::open(path) {
        let _ = f.take(96 * 1024 * 1024).read_to_end(&mut buf);
    }
    let hay = lower_ascii(&buf);
    s.findings = scan_bytes(&hay, size);

    let trust = crate::trust::check_file(&path.to_string_lossy());
    s.signer = trust.signer().map(String::from);
    match &trust {
        crate::trust::Trust::Signed { .. } => {}
        crate::trust::Trust::Unsigned => s.findings.push(Finding {
            level: Level::Caution,
            title: crate::i18n::t("Not digitally signed"),
            detail: crate::i18n::t("Nobody vouches for this file. Most reputable software is signed; download it from the publisher's own website if you can."),
        }),
        crate::trust::Trust::Invalid { reason, .. } => s.findings.push(Finding {
            level: Level::Danger,
            title: crate::i18n::t("Broken signature"),
            detail: crate::i18n::tf("The file's digital signature doesn't check out ({reason}). It may have been altered after it was signed.", &[("reason", crate::i18n::t(&reason.to_string()))]),
        }),
        _ => {}
    }
    s.source = download_source(path);
    if let Some(src) = &s.source {
        let l = src.to_lowercase();
        let mirrors = [
            "softonic",
            "cnet.com",
            "download.com",
            "uptodown",
            "filehippo",
            "softpedia",
            "malavida",
            "sourceforge.net/projects/",
        ];
        if let Some(m) = mirrors.iter().find(|m| l.contains(*m)) {
            s.findings.push(Finding {
                level: Level::Caution,
                title: crate::i18n::t("Downloaded from a download site"),
                detail: crate::i18n::tf("This came from {site} rather than the publisher. Some download sites wrap installers with their own offers.", &[("site", m.trim_end_matches('/').to_string())]),
            });
        }
    }
    if s.findings.is_empty() {
        s.findings.push(Finding {
            level: Level::Info,
            title: "No warning signs found".into(),
            detail: "Nothing known to bundle adware was found in the setup file. Still read each page of the installer.".into(),
        });
    }
    s.findings.sort_by_key(|f| std::cmp::Reverse(f.level));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_bundlers_offers_and_miners() {
        let mut hay = b"... OpenCandy ... Special Offer ... McAfee ... ".to_vec();
        hay.extend("xmrig".encode_utf16().flat_map(|u| u.to_le_bytes()));
        let f = scan_bytes(&lower_ascii(&hay), 50_000_000);
        let titles: Vec<&str> = f.iter().map(|x| x.title.as_str()).collect();
        assert!(titles.contains(&"Bundled adware: OpenCandy"), "{titles:?}");
        assert!(titles.contains(&"Coin miner"), "{titles:?}");
        assert!(
            titles.contains(&"Other software may be offered"),
            "{titles:?}"
        );
        assert!(titles.contains(&"Offer screen"), "{titles:?}");
    }

    #[test]
    fn clean_files_and_web_installers() {
        assert!(scan_bytes(b"nullsoft install system", 30_000_000).is_empty());
        let web = scan_bytes(b"inetc https://example.com/download/app.zip", 900_000);
        assert_eq!(web[0].title, "Downloads during setup");
    }
}
