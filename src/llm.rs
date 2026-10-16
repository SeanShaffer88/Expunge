//! A second opinion from a language model running on this PC, for the
//! background components the Decrapifier's rules couldn't judge.
//!
//! Talks to any OpenAI-compatible chat endpoint (Ollama, llama.cpp's
//! server, LM Studio) over plain HTTP, and only on this computer: an
//! address that isn't the loopback interface is refused, so nothing about
//! the PC leaves it. Answers are checked: anything malformed, or naming a
//! component that wasn't asked about, is ignored.

use crate::decrap::{Component, DecidedBy, Verdict};
use anyhow::{anyhow, bail, Context, Result};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Where the model is, e.g. `http://localhost:11434/v1/chat/completions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub path: String,
}

/// Parse an `http://` URL whose host is this computer.
pub fn parse_local_url(url: &str) -> Result<Endpoint> {
    let rest = url
        .trim()
        .strip_prefix("http://")
        .ok_or_else(|| anyhow!("the model's address must start with http://"))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/v1/chat/completions"),
    };
    let (host, port) = if let Some(h) = authority.strip_prefix('[') {
        let (h, p) = h
            .split_once(']')
            .ok_or_else(|| anyhow!("bad address: {url}"))?;
        (h.to_string(), p.strip_prefix(':').unwrap_or("80"))
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p),
            None => (authority.to_string(), "80"),
        }
    };
    let port: u16 = port.parse().map_err(|_| anyhow!("bad port in {url}"))?;
    let local = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false);
    if !local {
        bail!("only a model on this computer (localhost) can be used, so nothing leaves the PC");
    }
    Ok(Endpoint {
        host,
        port,
        path: path.to_string(),
    })
}

/// POST a JSON body and return the response body.
fn post_json(ep: &Endpoint, body: &str, timeout: Duration) -> Result<String> {
    let addr = if ep.host.contains(':') {
        format!("[{}]:{}", ep.host, ep.port)
    } else {
        format!("{}:{}", ep.host, ep.port)
    };
    let sock_addr = std::net::ToSocketAddrs::to_socket_addrs(&addr)?
        .find(|a| a.ip().is_loopback())
        .ok_or_else(|| anyhow!("{} is not on this computer", ep.host))?;
    let mut s = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(5))
        .with_context(|| format!("no model is answering at {addr}"))?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(Duration::from_secs(10)))?;
    write!(
        s,
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        ep.path,
        addr,
        body.len()
    )?;
    s.write_all(body.as_bytes())?;
    s.flush()?;
    let mut r = BufReader::new(s);
    let mut status = String::new();
    r.read_line(&mut status)?;
    let code: u16 = status
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| anyhow!("the model's reply wasn't HTTP"))?;
    let (mut length, mut chunked) = (None::<usize>, false);
    loop {
        let mut line = String::new();
        if r.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            if k.eq_ignore_ascii_case("content-length") {
                length = v.parse().ok();
            } else if k.eq_ignore_ascii_case("transfer-encoding")
                && v.to_ascii_lowercase().contains("chunked")
            {
                chunked = true;
            }
        }
    }
    const MAX: usize = 4 * 1024 * 1024;
    let mut body = Vec::new();
    if chunked {
        loop {
            let mut size = String::new();
            r.read_line(&mut size)?;
            let n = usize::from_str_radix(size.trim().split(';').next().unwrap_or("0"), 16)
                .map_err(|_| anyhow!("bad chunk in the model's reply"))?;
            if n == 0 {
                break;
            }
            if body.len() + n > MAX {
                bail!("the model's reply is too long");
            }
            let at = body.len();
            body.resize(at + n, 0);
            r.read_exact(&mut body[at..])?;
            let mut crlf = [0u8; 2];
            r.read_exact(&mut crlf)?;
        }
    } else if let Some(n) = length {
        if n > MAX {
            bail!("the model's reply is too long");
        }
        body.resize(n, 0);
        r.read_exact(&mut body)?;
    } else {
        r.take(MAX as u64).read_to_end(&mut body)?;
    }
    let text = String::from_utf8_lossy(&body).into_owned();
    if !(200..300).contains(&code) {
        bail!(
            "the model answered {code}: {}",
            text.chars().take(200).collect::<String>()
        );
    }
    Ok(text)
}

const INSTRUCTIONS: &str = "You review background components that a Windows program's installer added. \
For each component decide: \"required\" (the program needs it to work), \"optional\" (convenience, \
such as starting with Windows), or \"bloat\" (telemetry, crash or usage reporting, silent updaters, \
advertising, or software unrelated to the program). Answer with JSON only: an array of objects \
{\"id\": number, \"verdict\": \"required\"|\"optional\"|\"bloat\", \"reason\": short sentence}. \
If unsure, answer \"optional\".";

/// The request body asking about these components (by index into `asked`).
pub fn request_body(model: &str, app: &str, publisher: &str, asked: &[&Component]) -> String {
    let items: Vec<crate::json::Value> = asked
        .iter()
        .enumerate()
        .map(|(i, c)| {
            crate::json!({
                "id": i,
                "type": c.kind.label(),
                "name": c.name,
                "runs": c.command,
                "defined_in": c.location,
            })
        })
        .collect();
    let user = crate::json!({
        "program": app,
        "publisher": publisher,
        "components": items,
    });
    crate::json!({
        "model": model,
        "temperature": 0,
        "stream": false,
        "messages": [
            {"role": "system", "content": INSTRUCTIONS},
            {"role": "user", "content": user.to_string()},
        ],
    })
    .to_string()
}

/// One judgement from the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judgement {
    pub id: usize,
    pub verdict: Verdict,
    pub reason: String,
}

/// Read the model's answer out of a chat-completions response. Only
/// well-formed entries for ids that were asked about are kept.
pub fn parse_answer(response: &str, asked: usize) -> Vec<Judgement> {
    let Ok(v) = crate::json::compat::from_str::<crate::json::Value>(response) else {
        return Vec::new();
    };
    let content = v["choices"][0]["message"]["content"].as_str().unwrap_or("");
    // Models often wrap JSON in prose or a code fence: take the array.
    let (Some(a), Some(b)) = (content.find('['), content.rfind(']')) else {
        return Vec::new();
    };
    if b < a {
        return Vec::new();
    }
    let Ok(crate::json::Value::Array(items)) = crate::json::compat::from_str(&content[a..=b])
    else {
        return Vec::new();
    };
    let mut out: Vec<Judgement> = Vec::new();
    for it in items {
        let Some(id) = it["id"].as_u64().map(|i| i as usize) else {
            continue;
        };
        if id >= asked || out.iter().any(|j| j.id == id) {
            continue;
        }
        let verdict = match it["verdict"]
            .as_str()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("required") => Verdict::Required,
            Some("optional") => Verdict::Optional,
            Some("bloat") => Verdict::Bloat,
            _ => continue,
        };
        let reason: String = it["reason"]
            .as_str()
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect();
        out.push(Judgement {
            id,
            verdict,
            reason,
        });
    }
    out
}

/// Ask the model about every component the rules left *Unknown*, and
/// apply its answers. Returns how many it judged.
pub fn judge_unknowns(
    url: &str,
    model: &str,
    app: &str,
    publisher: &str,
    comps: &mut [Component],
) -> Result<usize> {
    let ep = parse_local_url(url)?;
    let idx: Vec<usize> = comps
        .iter()
        .enumerate()
        .filter(|(_, c)| c.verdict == Verdict::Unknown)
        .map(|(i, _)| i)
        .collect();
    if idx.is_empty() {
        return Ok(0);
    }
    let asked: Vec<&Component> = idx.iter().map(|&i| &comps[i]).collect();
    let body = request_body(model, app, publisher, &asked);
    let response = post_json(&ep, &body, Duration::from_secs(120))?;
    let answers = parse_answer(&response, idx.len());
    for j in &answers {
        let c = &mut comps[idx[j.id]];
        c.verdict = j.verdict;
        c.decided_by = DecidedBy::Model;
        if !j.reason.is_empty() {
            c.reasons.insert(0, format!("Local model: {}", j.reason));
        }
    }
    Ok(answers.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decrap::{Category, Kind};
    use std::net::TcpListener;

    #[test]
    fn only_this_computer_is_accepted() {
        let ep = parse_local_url("http://localhost:11434/v1/chat/completions").unwrap();
        assert_eq!(
            (ep.host.as_str(), ep.port, ep.path.as_str()),
            ("localhost", 11434, "/v1/chat/completions")
        );
        assert!(parse_local_url("http://127.0.0.1:8080").is_ok());
        assert!(parse_local_url("http://[::1]:8080/x").is_ok());
        for bad in [
            "http://example.com/v1/chat/completions",
            "http://192.168.1.5:11434",
            "https://localhost:11434",
            "http://localhost.evil.com",
            "localhost:11434",
        ] {
            assert!(parse_local_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn answers_are_checked() {
        let content = "Sure!\n```json\n[{\"id\":0,\"verdict\":\"bloat\",\"reason\":\"phones home\"},{\"id\":0,\"verdict\":\"required\"},{\"id\":7,\"verdict\":\"bloat\"},{\"id\":1,\"verdict\":\"maybe\"},{\"id\":1,\"verdict\":\"Optional\",\"reason\":\"tray\\u0007icon\"}]\n```";
        let resp = crate::json!({"choices":[{"message":{"content": content}}]}).to_string();
        let got = parse_answer(&resp, 2);
        assert_eq!(
            got,
            [
                Judgement {
                    id: 0,
                    verdict: Verdict::Bloat,
                    reason: "phones home".into()
                },
                Judgement {
                    id: 1,
                    verdict: Verdict::Optional,
                    reason: "trayicon".into()
                },
            ]
        );
        assert!(parse_answer("not json", 2).is_empty());
        assert!(parse_answer("{\"choices\":[]}", 2).is_empty());
    }

    fn comp(name: &str, verdict: Verdict) -> Component {
        Component {
            kind: Kind::Service,
            name: name.into(),
            command: format!("C:\\X\\{name}.exe"),
            location: format!("HKLM\\SYSTEM\\CurrentControlSet\\Services\\{name}"),
            verdict,
            category: Category::Other,
            reasons: vec![],
            decided_by: DecidedBy::Rules,
        }
    }

    #[test]
    fn unknowns_are_asked_and_answered_over_local_http() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut len = 0usize;
            loop {
                let mut l = String::new();
                r.read_line(&mut l).unwrap();
                if l.trim().is_empty() {
                    break;
                }
                if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let req: crate::json::Value = crate::json::compat::from_slice(&body).unwrap();
            let user = req["messages"][1]["content"].as_str().unwrap().to_string();
            let content = "[{\"id\":0,\"verdict\":\"bloat\",\"reason\":\"usage reporting\"}]";
            let reply = crate::json!({"choices":[{"message":{"content":content}}]}).to_string();
            // Chunked, as Ollama sends it.
            let (a, b) = reply.split_at(reply.len() / 2);
            write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
                a.len(),
                b.len()
            )
            .unwrap();
            user
        });
        let mut comps = vec![
            comp("Known", Verdict::Required),
            comp("Mystery", Verdict::Unknown),
        ];
        let n = judge_unknowns(
            &format!("http://127.0.0.1:{port}/v1/chat/completions"),
            "test-model",
            "App",
            "Vendor",
            &mut comps,
        )
        .unwrap();
        let asked = server.join().unwrap();
        assert_eq!(n, 1);
        assert!(
            asked.contains("Mystery") && !asked.contains("Known"),
            "only unknowns are sent: {asked}"
        );
        assert_eq!(comps[0].verdict, Verdict::Required);
        assert_eq!(comps[1].verdict, Verdict::Bloat);
        assert_eq!(comps[1].decided_by, DecidedBy::Model);
        assert!(comps[1].reasons[0].contains("usage reporting"));
    }
}
