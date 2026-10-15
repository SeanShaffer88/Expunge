//! Opt-in reputation check: look a file's SHA-256 hash up online. Only the
//! hash leaves the PC, never the file.
//!
//! CIRCL's public hashlookup service says whether the file is a known,
//! published file (from the NSRL and other catalogues); with a VirusTotal
//! API key, VirusTotal's detection counts are shown as well.

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// Antivirus engines flag it.
    Malicious { detections: u32, engines: u32 },
    /// Scanned by VirusTotal and not flagged.
    Clean { engines: u32 },
    /// Listed in a catalogue of published software.
    Known { source: String },
    /// Not found anywhere: not necessarily bad, just unknown.
    Unknown,
}

impl Verdict {
    pub fn is_bad(&self) -> bool {
        matches!(self, Verdict::Malicious { .. })
    }
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn virustotal_url(hash: &str) -> String {
    format!("https://www.virustotal.com/gui/file/{hash}")
}

/// GET a URL with Windows' built-in curl; returns (HTTP status, body).
fn get(url: &str, headers: &[&str]) -> Result<(u32, String), String> {
    let mut args: Vec<String> = vec![
        "-s".into(),
        "-m".into(),
        "20".into(),
        "-w".into(),
        "\n%{http_code}".into(),
        "-H".into(),
        "accept: application/json".into(),
    ];
    for h in headers {
        args.push("-H".into());
        args.push((*h).into());
    }
    args.push(url.into());
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let curl = if cfg!(windows) { "curl.exe" } else { "curl" };
    let (_, out) = crate::platform::run_hidden(curl, &refs).map_err(|e| e.to_string())?;
    let (body, code) = out.rsplit_once('\n').unwrap_or(("", out.as_str()));
    let code: u32 = code.trim().parse().map_err(|_| "no answer from the server".to_string())?;
    if code == 0 {
        return Err("couldn't reach the lookup service (is the PC online?)".into());
    }
    Ok((code, body.to_string()))
}

/// Look the hash up. `vt_key` adds VirusTotal's verdict.
pub fn check(hash: &str, vt_key: Option<&str>) -> Result<Verdict, String> {
    if let Some(key) = vt_key.filter(|k| !k.trim().is_empty()) {
        let (code, body) = get(
            &format!("https://www.virustotal.com/api/v3/files/{hash}"),
            &[&format!("x-apikey: {}", key.trim())],
        )?;
        match code {
            200 => {
                if let Some(v) = parse_virustotal(&body) {
                    return Ok(v);
                }
            }
            404 => {}
            401 | 403 => return Err("VirusTotal rejected the API key".into()),
            429 => return Err("VirusTotal's request limit was reached; try again in a minute".into()),
            c => return Err(format!("VirusTotal answered with error {c}")),
        }
    }
    let (code, body) = get(&format!("https://hashlookup.circl.lu/lookup/sha256/{hash}"), &[])?;
    match code {
        200 => Ok(parse_hashlookup(&body)),
        404 => Ok(Verdict::Unknown),
        c => Err(format!("the lookup service answered with error {c}")),
    }
}

pub fn parse_virustotal(json: &str) -> Option<Verdict> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let stats = v.pointer("/data/attributes/last_analysis_stats")?;
    let n = |k: &str| stats.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let bad = n("malicious") + n("suspicious");
    let engines = n("malicious") + n("suspicious") + n("undetected") + n("harmless");
    Some(if bad > 0 {
        Verdict::Malicious { detections: bad, engines }
    } else {
        Verdict::Clean { engines }
    })
}

pub fn parse_hashlookup(json: &str) -> Verdict {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let product = v
        .get("ProductCode")
        .and_then(|p| p.get("ProductName"))
        .and_then(|x| x.as_str())
        .or_else(|| v.get("FileName").and_then(|x| x.as_str()))
        .unwrap_or("")
        .to_string();
    let source = v.get("source").and_then(|x| x.as_str()).unwrap_or("hashlookup");
    Verdict::Known {
        source: if product.is_empty() {
            source.to_string()
        } else {
            format!("{product} ({source})")
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_answers() {
        let vt = r#"{"data":{"attributes":{"last_analysis_stats":{"malicious":3,"suspicious":1,"undetected":60,"harmless":0}}}}"#;
        assert_eq!(parse_virustotal(vt), Some(Verdict::Malicious { detections: 4, engines: 64 }));
        let ok = r#"{"data":{"attributes":{"last_analysis_stats":{"malicious":0,"undetected":70}}}}"#;
        assert_eq!(parse_virustotal(ok), Some(Verdict::Clean { engines: 70 }));
        let hl = r#"{"FileName":"7z.exe","source":"NSRL","ProductCode":{"ProductName":"7-Zip"}}"#;
        assert_eq!(parse_hashlookup(hl), Verdict::Known { source: "7-Zip (NSRL)".into() });
        let d = std::env::temp_dir().join(format!("expunge-rep-{}", std::process::id()));
        std::fs::write(&d, b"abc").unwrap();
        assert_eq!(
            sha256_file(&d).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_file(&d);
    }
}
