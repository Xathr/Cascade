//! The web editor's server: serves the editor page and its two requests (the current list, and
//! saving a changed one) on a port of its own. Every request for data must carry the secret
//! token from the link `/npc web` gives.

use crate::http::{self, Parsed};
use crate::net::{Event, Handler, Reactor, Token};
use std::collections::HashMap;
use std::net::SocketAddr;

const PAGE: &str = include_str!("../web/editor.html");

/// What the editor asks of the plugin.
pub enum Ask {
    /// The current list and the choices the editor offers, as JSON.
    State,
    /// Replace the list with this JSON.
    Save(String),
}

pub struct Web {
    reactor: Reactor,
    /// What has arrived so far on each connection.
    waiting: HashMap<Token, Vec<u8>>,
    pub token: String,
    pub port: u16,
}

/// A fresh secret for the editor's link. The standard hasher is seeded from the system's own
/// randomness, differently every time.
fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    (0..3).map(|i| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i);
        format!("{:016x}", h.finish())
    }).collect::<String>()[..40].to_string()
}

impl Web {
    pub fn start(port: u16) -> Result<Web, String> {
        let mut reactor = Reactor::new();
        reactor.listen(SocketAddr::from(([0, 0, 0, 0], port)), 16)?;
        Ok(Web { reactor, waiting: HashMap::new(), token: new_token(), port })
    }

    /// Serves what has come in, for at most a couple of milliseconds. `answer` gives the JSON
    /// reply to each request for data.
    pub fn serve(&mut self, answer: &mut dyn FnMut(Ask) -> String) {
        let Web { reactor, waiting, token, .. } = self;
        let mut session = Session { waiting, token, answer };
        reactor.run_for(&mut session, 2);
    }
}

struct Session<'a> {
    waiting: &'a mut HashMap<Token, Vec<u8>>,
    token: &'a str,
    answer: &'a mut dyn FnMut(Ask) -> String,
}

impl Session<'_> {
    fn respond(&mut self, request: http::Request) -> Vec<u8> {
        const JSON: &str = "application/json; charset=utf-8";
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/" | "/index.html") => http::reply("200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
            (_, path) if path.starts_with("/api/") => {
                let given = request.query.iter().find(|(name, _)| name == "t").map_or("", |(_, value)| value.as_str());
                if !http::token_matches(given, self.token) {
                    return http::reply("403 Forbidden", JSON, br#"{"ok":false,"error":"this link is out of date"}"#);
                }
                match (request.method.as_str(), path) {
                    ("GET", "/api/state") => http::reply("200 OK", JSON, (self.answer)(Ask::State).as_bytes()),
                    ("POST", "/api/save") => match String::from_utf8(request.body) {
                        Ok(body) => http::reply("200 OK", JSON, (self.answer)(Ask::Save(body)).as_bytes()),
                        Err(_) => http::reply("400 Bad Request", JSON, br#"{"ok":false,"error":"not text"}"#),
                    },
                    _ => http::reply("404 Not Found", JSON, br#"{"ok":false,"error":"no such request"}"#),
                }
            }
            _ => http::reply("404 Not Found", "text/plain", b"not found"),
        }
    }
}

impl Handler for Session<'_> {
    fn handle(&mut self, reactor: &mut Reactor, event: Event) {
        match event {
            Event::Accepted { token, .. } => {
                // a few connections at a time is plenty for an editor
                if self.waiting.len() >= 32 {
                    reactor.close_now(token);
                } else {
                    self.waiting.insert(token, Vec::new());
                }
            }
            Event::Data { token, data } => {
                let Some(buffer) = self.waiting.get_mut(&token) else { return };
                buffer.extend_from_slice(&data);
                if buffer.len() > http::MAX_REQUEST + 16 * 1024 {
                    self.waiting.remove(&token);
                    reactor.close_now(token);
                    return;
                }
                let reply = match http::parse(buffer) {
                    Parsed::Incomplete => return,
                    Parsed::Bad(why) => http::reply("400 Bad Request", "text/plain", why.as_bytes()),
                    Parsed::Ready(request) => self.respond(request),
                };
                self.waiting.remove(&token);
                reactor.send(token, &reply);
                reactor.close_gracefully(token);
            }
            Event::Closed { token, .. } => {
                self.waiting.remove(&token);
            }
            _ => {}
        }
    }
}
