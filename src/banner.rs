//! The startup banner printed to the console when the plugin loads.

use crate::color;

/// The cascade logo, three rows tall. Each row is split after "cas" so the halves can be
/// coloured differently.
const LOGO: [(&str, &str); 3] = [
    ("┌─┐┌─┐┌─┐", "┌─┐┌─┐┌┬┐┌─┐"),
    ("│  ├─┤└─┐", "│  ├─┤ ││├┤ "),
    ("└─┘┴ ┴└─┘", "└─┘┴ ┴─┴┘└─┘"),
];

/// What the banner reports.
pub struct Info<'a> {
    pub version: &'a str,
    pub npcs: usize,
    pub displays: usize,
    pub file: &'a str,
}

fn count(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

/// The banner, one console line per entry.
pub fn lines(info: &Info) -> Vec<String> {
    let ribbon = format!("{} {}{}{} {}", color::value("NPCs"), color::paint("95", "◆"), color::dim("━━━━━━━━━━"), color::paint("95", "◆"), color::value("displays"));
    let beside = [String::new(), format!("{} {}", color::name("cascade"), color::dim(&format!("v{}", info.version))), ribbon];
    let mut out: Vec<String> = LOGO
        .iter()
        .zip(beside)
        .map(|((cas, cade), side)| {
            let row = format!("  {}{}", color::value(cas), color::paint("94", cade));
            if side.is_empty() { row } else { format!("{row}   {side}") }
        })
        .collect();
    for (key, value) in [
        ("NPCs", color::value(&count(info.npcs, "NPC"))),
        ("displays", color::value(&count(info.displays, "display"))),
        ("saved in", color::dim(info.file)),
        ("commands", format!("{} {}", color::name("/npc help"), color::name("/display help"))),
    ] {
        out.push(format!("  {} {value}", color::dim(&format!("{key:<13}"))));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_shows_the_logo_counts_and_file() {
        color::set_enabled(false);
        let lines = lines(&Info { version: "1.2.3", npcs: 1, displays: 4, file: "plugins/data/cascade/things.json" });
        color::set_enabled(true);
        assert_eq!(lines.len(), 7);
        assert_eq!(lines[0], "  ┌─┐┌─┐┌─┐┌─┐┌─┐┌┬┐┌─┐");
        assert_eq!(lines[1], "  │  ├─┤└─┐│  ├─┤ ││├┤    cascade v1.2.3");
        assert!(lines[2].ends_with("NPCs ◆━━━━━━━━━━◆ displays"));
        assert_eq!(lines[3], "  NPCs          1 NPC");
        assert_eq!(lines[4], "  displays      4 displays");
        assert_eq!(lines[5], "  saved in      plugins/data/cascade/things.json");
        // every logo row is the same width
        assert!(LOGO.iter().all(|(a, b)| a.chars().count() == 9 && b.chars().count() == 12));
    }
}
