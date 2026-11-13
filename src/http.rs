//! Just enough HTTP for the web editor: reading one request and writing one reply.

/// A request, once all of it has arrived.
#[derive(Debug, PartialEq)]
pub struct Request {
    pub method: String,
    pub path: String,
    /// What follows `?` in the address, as (name, value) pairs.
    pub query: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
    /// More bytes are needed.
    Incomplete,
    /// Not a request this server answers.
    Bad(&'static str),
    Ready(Request),
}

/// The most a request may be: the editor sends the whole list of NPCs and displays at once.
pub const MAX_REQUEST: usize = 2 * 1024 * 1024;

pub fn parse(buf: &[u8]) -> Parsed {
    let Some(head_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        return if buf.len() > 16 * 1024 { Parsed::Bad("request head too long") } else { Parsed::Incomplete };
    };
    let Ok(head) = std::str::from_utf8(&buf[..head_end]) else { return Parsed::Bad("request is not text") };
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (Some(method), Some(target)) = (first.next(), first.next()) else { return Parsed::Bad("no request line") };
    let mut length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                match value.trim().parse() {
                    Ok(n) => length = n,
                    Err(_) => return Parsed::Bad("bad content length"),
                }
            }
        }
    }
    if length > MAX_REQUEST {
        return Parsed::Bad("request too large");
    }
    let body_start = head_end + 4;
    if buf.len() < body_start + length {
        return Parsed::Incomplete;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let query = query.split('&').filter(|p| !p.is_empty()).map(|pair| {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        (name.to_string(), value.to_string())
    });
    Parsed::Ready(Request { method: method.to_string(), path: path.to_string(), query: query.collect(), body: buf[body_start..body_start + length].to_vec() })
}

/// A whole reply, after which the connection is closed.
pub fn reply(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

/// Whether `given` is the secret `token`, compared without stopping at the first difference.
pub fn token_matches(given: &str, token: &str) -> bool {
    given.len() == token.len() && !token.is_empty() && given.bytes().zip(token.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_request_when_all_of_it_is_there() {
        assert_eq!(parse(b"GET /api/state?t=abc&x=1 HTTP/1.1\r\nHost: x\r\n"), Parsed::Incomplete);
        let Parsed::Ready(r) = parse(b"GET /api/state?t=abc&x=1 HTTP/1.1\r\nHost: x\r\n\r\n") else { panic!() };
        assert_eq!((r.method.as_str(), r.path.as_str(), r.body.len()), ("GET", "/api/state", 0));
        assert_eq!(r.query, [("t".to_string(), "abc".to_string()), ("x".to_string(), "1".to_string())]);
        let post = b"POST /api/save?t=abc HTTP/1.1\r\ncontent-length: 7\r\n\r\n{\"a\":1}";
        assert_eq!(parse(&post[..post.len() - 2]), Parsed::Incomplete);
        let Parsed::Ready(r) = parse(post) else { panic!() };
        assert_eq!((r.method.as_str(), r.body.as_slice()), ("POST", &b"{\"a\":1}"[..]));
        assert_eq!(parse(b"POST / HTTP/1.1\r\nContent-Length: 99999999\r\n\r\n"), Parsed::Bad("request too large"));
        assert_eq!(parse(b"\r\n\r\n"), Parsed::Bad("no request line"));
        assert!(matches!(parse(&[b'a'; 20000]), Parsed::Bad(_)));
    }

    #[test]
    fn replies_and_tokens() {
        let r = String::from_utf8(reply("200 OK", "application/json", b"{}")).unwrap();
        assert!(r.starts_with("HTTP/1.1 200 OK\r\n") && r.contains("Content-Length: 2\r\n") && r.ends_with("\r\n\r\n{}"));
        assert!(token_matches("abc123", "abc123"));
        assert!(!token_matches("abc124", "abc123") && !token_matches("abc", "abc123") && !token_matches("", ""));
    }
}
