//! ANSI colours for console messages, like Pumpkin's own output. The web panel and log files that
//! strip ANSI codes show plain text. Turned off with `console_colors = false` in config.toml.
//! Chat messages from the plugin use the same colours.

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(true);

/// The plugin's orange (a 256-colour code; chat shows it as gold).
const ORANGE: &str = "38;5;208";

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Any ANSI colour code.
pub fn paint(code: &str, s: &str) -> String {
    if ENABLED.load(Ordering::Relaxed) { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
}

/// Text from a player's client, made safe to print: control characters (escape codes, line
/// breaks) are dropped so it cannot colour, clear or forge console lines, and it is cut to `max`
/// characters.
pub fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// `s` without ANSI colour codes, for chat (which shows them as garbage).
pub fn strip(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // skip "[...m"
            for d in chars.by_ref() {
                if d == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Runs `f` with colours on, whatever `console_colors` says. For text that is turned into chat
/// colours afterwards rather than printed.
pub fn with_colours<T>(f: impl FnOnce() -> T) -> T {
    let was = ENABLED.swap(true, Ordering::Relaxed);
    let out = f();
    ENABLED.store(was, Ordering::Relaxed);
    out
}

/// The chat colour for an ANSI colour code used in this file, and whether it is bold.
fn chat_colour(code: &str) -> (&'static str, bool) {
    match code {
        "1;97" => ("white", true),
        "96" => ("aqua", false),
        "90" => ("gray", false),
        "92" => ("green", false),
        "93" => ("yellow", false),
        "91" => ("red", false),
        "94" => ("blue", false),
        ORANGE => ("gold", false),
        _ => ("white", false),
    }
}

/// A reply written with `&` codes and buttons (see `commands::button`), as a console line: the
/// buttons are left out.
pub fn console(line: &str) -> String {
    let mut plain = String::new();
    let mut rest = line;
    while let Some(start) = rest.find('\u{1}') {
        plain.push_str(&rest[..start]);
        rest = rest[start..].split_once('\u{3}').map_or("", |(_, after)| after);
    }
    plain.push_str(rest);
    styled(plain.trim_end())
}

/// The same reply as a chat message (a JSON text component): the colours become chat colours
/// and each button a piece of text to click. `tag` goes in front, in the plugin's colour.
pub fn chat(line: &str, tag: Option<&str>) -> String {
    let mut parts: Vec<serde_json::Value> = Vec::new();
    if let Some(tag) = tag {
        parts.push(serde_json::json!({ "text": format!("{tag} "), "color": "gold" }));
    }
    let mut rest = line;
    loop {
        let (before, button) = match rest.find('\u{1}') {
            Some(start) => (&rest[..start], rest[start + 1..].split_once('\u{3}')),
            None => (rest, None),
        };
        parts.extend(chat_parts(&with_colours(|| styled(before))));
        let Some((button, after)) = button else { break };
        let mut fields = button.split('\u{2}');
        let (label, action, hover) = (fields.next().unwrap_or(""), fields.next().unwrap_or(""), fields.next().unwrap_or(""));
        let (kind, target) = action.split_at(action.len().min(1));
        // a command to finish typing reads as the command itself; anything else is a button
        let (text, colour) = if label.starts_with('/') { (format!("{label} "), "yellow") } else { (format!("[{label}] "), "gold") };
        let click = match kind {
            "r" => serde_json::json!({ "action": "run_command", "command": target }),
            "u" => serde_json::json!({ "action": "open_url", "url": target }),
            _ => serde_json::json!({ "action": "suggest_command", "command": target }),
        };
        parts.push(serde_json::json!({ "text": text, "color": colour, "click_event": click, "hover_event": { "action": "show_text", "value": [{ "text": hover, "color": "gray" }] } }));
        rest = after;
    }
    serde_json::json!({ "text": "", "extra": parts }).to_string()
}

/// An ANSI-coloured console line as a chat message (a JSON text component) with the same colours.
/// Links are clickable.
pub fn chat_json(line: &str) -> String {
    serde_json::json!({ "text": "", "extra": chat_parts(line) }).to_string()
}

fn chat_parts(line: &str) -> Vec<serde_json::Value> {
    let mut parts = Vec::new();
    let (mut colour, mut bold) = ("white", false);
    let mut rest = line;
    let mut push = |text: &str, colour: &str, bold: bool| {
        if text.is_empty() {
            return;
        }
        let mut part = serde_json::json!({ "text": text, "color": colour });
        if bold {
            part["bold"] = serde_json::json!(true);
        }
        if text.starts_with("https://") && !text.contains(' ') {
            part["click_event"] = serde_json::json!({ "action": "open_url", "url": text });
            part["hover_event"] = serde_json::json!({ "action": "show_text", "value": [{ "text": "Click to open", "color": "gray" }] });
        }
        parts.push(part);
    };
    while let Some(start) = rest.find("\x1b[") {
        push(&rest[..start], colour, bold);
        let after = &rest[start + 2..];
        let Some(end) = after.find('m') else {
            rest = "";
            break;
        };
        (colour, bold) = chat_colour(&after[..end]);
        rest = &after[end + 1..];
    }
    push(rest, colour, bold);
    drop(push);
    parts
}

/// Names: of NPCs, displays and players.
pub fn name(s: &str) -> String { paint("1;97", s) }
/// Values and settings.
pub fn value(s: &str) -> String { paint("93", s) }
/// Places, hints and other detail.
pub fn dim(s: &str) -> String { paint("90", s) }
/// Things that are on, or went well.
pub fn good(s: &str) -> String { paint("92", s) }
/// Notes and warnings.
pub fn warn(s: &str) -> String { paint("93", s) }
/// Failures.
pub fn bad(s: &str) -> String { paint("91", s) }
/// The plugin's tag.
pub fn tag(s: &str) -> String { paint(ORANGE, s) }
/// Orange, for the second half of the logo and other accents.
pub fn orange(s: &str) -> String { paint(ORANGE, s) }

/// A message written with `&` colour codes, in the console colours above. The codes are used by
/// meaning: `&e` a name, `&b` a value, `&7` detail, `&a` good, `&6` a note, `&c` bad, `&f` plain.
pub fn styled(line: &str) -> String {
    let mut out = String::new();
    let mut code = "";
    let mut text = String::new();
    let flush = |code: &str, text: &mut String, out: &mut String| {
        if !text.is_empty() {
            // spaces at the end stay outside the colour, so words are coloured, not gaps
            let word = text.trim_end();
            out.push_str(&if code.is_empty() || word.is_empty() { word.to_string() } else { paint(code, word) });
            out.push_str(&text[word.len()..]);
            text.clear();
        }
    };
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        let next = match (c, chars.peek()) {
            ('&', Some('e')) => Some("1;97"),
            ('&', Some('b')) => Some("93"),
            ('&', Some('7' | '8')) => Some("90"),
            ('&', Some('a')) => Some("92"),
            ('&', Some('6')) => Some("93"),
            ('&', Some('c')) => Some("91"),
            ('&', Some('f' | 'r')) => Some(""),
            _ => None,
        };
        match next {
            Some(next) => {
                chars.next();
                flush(code, &mut text, &mut out);
                code = next;
            }
            None => text.push(c),
        }
    }
    flush(code, &mut text, &mut out);
    out
}

/// Tests that switch colours on or off hold this, so they do not disturb each other.
#[cfg(test)]
pub fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn styled_messages_become_console_and_chat_colours() {
        let _colours = test_lock();
        set_enabled(true);
        let line = styled("&fcreated NPC &eguide &7(mob villager)&f: look &aon");
        assert_eq!(line, "created NPC \x1b[1;97mguide\x1b[0m \x1b[90m(mob villager)\x1b[0m: look \x1b[92mon\x1b[0m");
        assert_eq!(strip(&line), "created NPC guide (mob villager): look on");
        let chat = chat_json(&format!("{} {line}", tag("[Cascade]")));
        assert!(chat.contains(r#"{"color":"gold","text":"[Cascade]"}"#), "{chat}");
        assert!(chat_json(&styled("&b5")).contains(r#"{"color":"yellow","text":"5"}"#));
        assert!(chat.contains(r#"{"bold":true,"color":"white","text":"guide"}"#), "{chat}");
        assert!(chat.contains(r#"{"color":"green","text":"on"}"#), "{chat}");
        // a message someone wrote for players keeps its own codes
        assert_eq!(styled("&dpink &lbold"), "&dpink &lbold");
    }

    #[test]
    fn buttons_are_clickable_in_chat_and_absent_from_the_console() {
        let _colours = test_lock();
        set_enabled(false);
        let line = format!(" &8\u{2022} &eguide&7 (mob villager) {}{}", "\u{1}go\u{2}r/npc tp guide\u{2}Teleport to it\u{3}", "\u{1}remove\u{2}s/npc remove guide\u{2}Delete\u{3}");
        assert_eq!(console(&line), " \u{2022} guide (mob villager)");
        let chat: serde_json::Value = serde_json::from_str(&chat(&line, Some("[Cascade]"))).unwrap();
        let parts = chat["extra"].as_array().unwrap();
        assert_eq!(parts[0], serde_json::json!({ "text": "[Cascade] ", "color": "gold" }));
        let go = parts.iter().find(|p| p["text"] == "[go] ").unwrap();
        assert_eq!(go["click_event"], serde_json::json!({ "action": "run_command", "command": "/npc tp guide" }));
        assert_eq!(go["hover_event"]["value"][0]["text"], "Teleport to it");
        let remove = parts.iter().find(|p| p["text"] == "[remove] ").unwrap();
        assert_eq!(remove["click_event"]["action"], "suggest_command");
        assert!(parts.iter().any(|p| p["text"] == "guide" && p["bold"] == true), "colours are on in chat whatever the console setting");
        set_enabled(true);
        let link: serde_json::Value = serde_json::from_str(&super::chat("\u{1}open\u{2}uhttp://x:1/?t=a\u{2}Open\u{3}", None)).unwrap();
        assert_eq!(link["extra"][0]["click_event"], serde_json::json!({ "action": "open_url", "url": "http://x:1/?t=a" }));
    }

    #[test]
    fn colours_and_plain() {
        let _colours = test_lock();
        set_enabled(true);
        assert_eq!(good("joined"), "\x1b[92mjoined\x1b[0m");
        set_enabled(false);
        assert_eq!(good("joined"), "joined");
        set_enabled(true);
    }
    #[test]
    fn console_lines_become_coloured_chat() {
        let _colours = test_lock();
        let line = with_colours(|| format!("{} {} {}", warn("v1.2 is out"), dim("Pumpkin Market:"), value("https://market.pumpkinmc.org/plugin/x")));
        let v: serde_json::Value = serde_json::from_str(&chat_json(&line)).unwrap();
        let extra = v["extra"].as_array().unwrap();
        assert_eq!(extra[0], serde_json::json!({ "text": "v1.2 is out", "color": "yellow" }));
        assert_eq!(extra[1], serde_json::json!({ "text": " ", "color": "white" }));
        assert_eq!(extra[2]["color"], "gray");
        let link = extra.last().unwrap();
        assert_eq!(link["color"], "yellow");
        assert_eq!(link["click_event"]["url"], "https://market.pumpkinmc.org/plugin/x");
        // bold names, and text with no codes at all
        let v: serde_json::Value = serde_json::from_str(&chat_json(&with_colours(|| name("Steve")))).unwrap();
        assert_eq!(v["extra"][0], serde_json::json!({ "text": "Steve", "color": "white", "bold": true }));
        let v: serde_json::Value = serde_json::from_str(&chat_json("plain")).unwrap();
        assert_eq!(v["extra"][0]["text"], "plain");
    }
    #[test]
    fn with_colours_restores_the_setting() {
        let _colours = test_lock();
        set_enabled(false);
        assert_eq!(with_colours(|| good("x")), "\x1b[92mx\x1b[0m");
        assert_eq!(good("x"), "x");
        set_enabled(true);
    }
    #[test]
    fn strip_removes_colour_codes() {
        let _colours = test_lock();
        assert_eq!(strip("\x1b[92mjoined\x1b[0m as \x1b[1;97mSteve\x1b[0m"), "joined as Steve");
        assert_eq!(strip("plain"), "plain");
    }
    #[test]
    fn client_text_cannot_forge_console_lines() {
        let _colours = test_lock();
        assert_eq!(clean("Steve\x1b[2J\n[Server] ops: Steve", 64), "Steve[2J[Server] ops: Steve");
        assert_eq!(clean("\u{9b}31mred\r", 64), "31mred");
        assert_eq!(clean("abcdef", 3), "abc");
    }
}
