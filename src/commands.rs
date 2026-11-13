//! What `/npc` and `/display` do to the saved list. This part knows nothing of the server, so
//! it can be tested on its own; the plugin carries out what it reports.

use crate::model::*;
use crate::names::{ENTITY_TYPES, PARTICLES};

/// What the command needs to know about whoever typed it and about the game.
pub trait Env {
    /// Where the sender stands: world, x, y, z, yaw, pitch. `None` for the console.
    fn here(&self) -> Option<(String, f64, f64, f64, f32, f32)>;
    /// The item in the sender's hand.
    fn held_item(&self) -> Option<String>;
    fn is_item(&self, id: &str) -> bool;
    fn is_block(&self, id: &str) -> bool;
}

#[derive(Default, Debug, PartialEq)]
pub struct Outcome {
    /// Lines to show the sender, with `&` colours.
    pub lines: Vec<String>,
    /// Things to build again in the world (their keys).
    pub changed: Vec<String>,
    /// Things to take out of the world.
    pub removed: Vec<String>,
    /// Send the sender here.
    pub teleport: Option<(String, f64, f64, f64)>,
    /// Read the file again.
    pub reload: bool,
}

/// Entity types that make no sense as an NPC's body.
const NOT_BODIES: &[&str] = &[
    "player", "item", "block_display", "item_display", "text_display", "interaction", "marker", "lightning_bolt", "fishing_bobber", "area_effect_cloud", "experience_orb", "falling_block", "tnt", "ender_dragon", "wither", "leash_knot", "painting", "item_frame", "glow_item_frame", "eye_of_ender",
];

fn clean_name(text: &str) -> String {
    let lower = text.trim().to_lowercase().replace(' ', "_");
    lower.strip_prefix("minecraft:").unwrap_or(&lower).to_string()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-'))
}

/// `on`, `off`, or nothing to flip.
fn toggle(now: bool, word: Option<&str>) -> Result<bool, String> {
    match word.map(str::to_lowercase).as_deref() {
        None | Some("toggle") => Ok(!now),
        Some("on" | "true" | "yes") => Ok(true),
        Some("off" | "false" | "no") => Ok(false),
        Some(other) => Err(format!("expected on or off, not {other}")),
    }
}

fn number<T: std::str::FromStr>(word: Option<&str>, what: &str) -> Result<T, String> {
    let word = word.ok_or(format!("give {what}"))?;
    word.parse().map_err(|_| format!("{word} is not a number for {what}"))
}

fn on_off(value: bool) -> &'static str {
    if value { "&aon" } else { "&coff" }
}

fn lines_of(text: &str) -> Vec<String> {
    text.split('|').map(|l| l.trim().to_string()).collect()
}

pub fn help(group: Group) -> Vec<String> {
    let c = group.command();
    let mut lines = vec![format!("&b/{c} &fcommands &7(<id> is a short name you choose)")];
    let create = match group {
        Group::Npc => "create <id> <player [skin]|block <block>|item <item>|any mob type>",
        Group::Display => "create <id> <text <words>|item <item>|block <block>|particle <name>>",
    };
    let mut rows = vec![
        (create, "make one where you stand"),
        ("remove <id>", "delete it"),
        ("list / info <id>", "see what exists"),
        ("movehere <id> / tp <id>", "bring it to you, or go to it"),
        ("rotate <id> <yaw> [pitch]", "turn it"),
        ("text <id> <line|line|...>", "set its name or text; | starts a new line"),
        ("addline / setline <n> / removeline <n>", "edit one line"),
        ("glow / scale <n>", "outline, size"),
        ("action <id> add <any|left|right> <what> <value>", "what a click does: message, broadcast, console, player, sound, teleport, title, actionbar"),
        ("action <id> list / remove <n> / clear", "see or drop actions"),
        ("cooldown <id> <seconds> / permission <id> <node|none>", "limit clicks"),
    ];
    match group {
        Group::Npc => rows.extend([
            ("look <id> [on|off] [blocks]", "face the nearest player"),
            ("skin <id> <player name> / type <id> <mob>", "change how it looks"),
            ("equip <id> <mainhand|offhand|head|chest|legs|feet> [item|hand|none]", "give it gear"),
            ("baby / sneak / invisible <id>", "pose"),
        ]),
        Group::Display => rows.extend([
            ("item / block / particle <id> <name>", "change what is shown"),
            ("billboard <id> <center|fixed|vertical|horizontal>", "how it turns to the viewer"),
            ("background <id> <#AARRGGBB|none> / shadow / seethrough", "text look"),
            ("width <id> <n> / align <id> <center|left|right>", "text layout"),
            ("spin <id> <degrees per second> / range <id> <n>", "turning, view distance"),
            ("count / interval / spread / speed <id> <n>", "particle settings"),
        ]),
    }
    rows.push(("reload", "read things.json again"));
    lines.extend(rows.iter().map(|(usage, what)| format!("&e/{c} {usage} &7{what}")));
    lines
}

pub fn run(store: &mut Store, group: Group, input: &str, env: &dyn Env) -> Outcome {
    let mut out = Outcome::default();
    if let Err(problem) = run_inner(store, group, input.trim(), env, &mut out) {
        out.lines.push(format!("&c{problem}"));
    }
    out
}

fn run_inner(store: &mut Store, group: Group, input: &str, env: &dyn Env, out: &mut Outcome) -> Result<(), String> {
    let (sub, rest) = input.split_once(char::is_whitespace).unwrap_or((input, ""));
    let rest = rest.trim();
    let sub = sub.to_lowercase();
    let word = group.word();
    match sub.as_str() {
        "" | "help" | "?" => {
            out.lines = help(group);
            return Ok(());
        }
        "list" => {
            let all: Vec<&Thing> = store.things.iter().filter(|t| t.group == group).collect();
            out.lines.push(format!("&b{} {word}{}", all.len(), if all.len() == 1 { "" } else { "s" }));
            out.lines.extend(all.iter().map(|t| format!("&e{}", t.summary())));
            return Ok(());
        }
        "reload" => {
            out.reload = true;
            return Ok(());
        }
        "create" => return create(store, group, rest, env, out),
        _ => {}
    }
    // everything else names a thing first
    let (id, rest) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let rest = rest.trim();
    let id = id.to_lowercase();
    if id.is_empty() {
        return Err(format!("which {word}? /{} {sub} <id>", group.command()));
    }
    let key = key(group, &id);
    if sub == "remove" || sub == "delete" {
        if !store.remove(group, &id) {
            return Err(format!("there is no {word} called {id}"));
        }
        out.removed.push(key);
        out.lines.push(format!("&fremoved &e{id}"));
        return Ok(());
    }
    let Some(thing) = store.get_mut(group, &id) else {
        return Err(format!("there is no {word} called {id}; /{} list shows them", group.command()));
    };
    let mut words = rest.split_whitespace();
    let first = words.next();
    let done = |out: &mut Outcome, line: String| {
        out.changed.push(key.clone());
        out.lines.push(line);
    };
    match sub.as_str() {
        "info" => {
            out.lines.push(format!("&b{}", thing.summary()));
            if !thing.lines.is_empty() {
                out.lines.push(format!("&7text: &f{}", thing.lines.join(" &8| &f")));
            }
            out.lines.push(format!("&7look {}&7, glow {}&7, scale &f{}&7, cooldown &f{}s&7, permission &f{}", on_off(thing.look), on_off(thing.glow), thing.scale, thing.cooldown, if thing.permission.is_empty() { "none" } else { &thing.permission }));
            if !thing.equipment.is_empty() {
                out.lines.push(format!("&7gear: &f{}", thing.equipment.iter().map(|(slot, item)| format!("{slot}={item}")).collect::<Vec<_>>().join(", ")));
            }
            for (i, action) in thing.actions.iter().enumerate() {
                out.lines.push(format!("&7action {}: &f{} click, {}: {}", i + 1, format!("{:?}", action.click).to_lowercase(), format!("{:?}", action.what).to_lowercase(), action.value));
            }
        }
        "tp" | "goto" => out.teleport = Some((thing.world.clone(), thing.x, thing.y, thing.z)),
        "movehere" | "move" => {
            let (world, x, y, z, yaw, _) = env.here().ok_or("only a player can do that")?;
            (thing.world, thing.x, thing.y, thing.z, thing.yaw) = (world, x, y, z, yaw);
            done(out, format!("&fmoved &e{id} &fto you"));
        }
        "rotate" => {
            thing.yaw = number(first, "the yaw, in degrees")?;
            if let Some(pitch) = words.next() {
                thing.pitch = number(Some(pitch), "the pitch")?;
            }
            done(out, format!("&e{id} &fnow faces {} {}", thing.yaw, thing.pitch));
        }
        "text" | "name" => {
            thing.lines = if rest.is_empty() || rest.eq_ignore_ascii_case("none") { Vec::new() } else { lines_of(rest) };
            done(out, format!("&e{id} &fnow shows {} line{}", thing.lines.len(), if thing.lines.len() == 1 { "" } else { "s" }));
        }
        "addline" => {
            thing.lines.push(rest.to_string());
            done(out, format!("&fadded line {} to &e{id}", thing.lines.len()));
        }
        "setline" | "removeline" => {
            let n: usize = number(first, "the line's number")?;
            if n == 0 || n > thing.lines.len() {
                return Err(format!("{id} has {} line{}", thing.lines.len(), if thing.lines.len() == 1 { "" } else { "s" }));
            }
            if sub == "removeline" {
                thing.lines.remove(n - 1);
            } else {
                thing.lines[n - 1] = rest.split_once(char::is_whitespace).map_or("", |(_, text)| text.trim()).to_string();
            }
            done(out, format!("&fline {n} of &e{id} &f{}", if sub == "removeline" { "removed" } else { "changed" }));
        }
        "glow" | "look" | "baby" | "sneak" | "invisible" | "shadow" | "seethrough" => {
            let field = match sub.as_str() {
                "glow" => &mut thing.glow,
                "look" => &mut thing.look,
                "baby" => &mut thing.baby,
                "sneak" => &mut thing.sneaking,
                "invisible" => &mut thing.invisible,
                "shadow" => &mut thing.shadow,
                _ => &mut thing.see_through,
            };
            *field = toggle(*field, first)?;
            let now = *field;
            if sub == "look" {
                if let Some(range) = words.next() {
                    thing.look_range = number(Some(range), "the distance")?;
                }
            }
            done(out, format!("&e{id}&f: {sub} {}", on_off(now)));
        }
        "scale" | "spin" | "range" | "spread" | "speed" | "cooldown" => {
            let n: f32 = number(first, &format!("a number for {sub}"))?;
            match sub.as_str() {
                "scale" if !(0.05..=16.0).contains(&n) => return Err("scale must be between 0.05 and 16".into()),
                "scale" => thing.scale = n,
                "spin" => thing.spin = n,
                "range" => thing.view_range = n.max(0.0),
                "spread" => thing.spread = n.max(0.0),
                "speed" => thing.speed = n.max(0.0),
                _ => thing.cooldown = n.max(0.0),
            }
            done(out, format!("&e{id}&f: {sub} &b{n}"));
        }
        "count" | "interval" | "width" => {
            let n: i32 = number(first, &format!("a whole number for {sub}"))?;
            match sub.as_str() {
                "count" => thing.count = n.clamp(0, 500),
                "interval" => thing.interval = n.max(1) as u32,
                _ => thing.line_width = n.max(1),
            }
            done(out, format!("&e{id}&f: {sub} &b{n}"));
        }
        "billboard" | "align" => {
            let value = first.unwrap_or("").to_lowercase();
            let allowed = if sub == "billboard" { BILLBOARDS } else { ALIGNMENTS };
            if !allowed.contains(&value.as_str()) {
                return Err(format!("{sub} is one of: {}", allowed.join(", ")));
            }
            if sub == "billboard" { thing.billboard = value.clone() } else { thing.alignment = value.clone() }
            done(out, format!("&e{id}&f: {sub} &b{value}"));
        }
        "background" => {
            let value = first.unwrap_or("none");
            if matches!(value.to_lowercase().as_str(), "none" | "default") {
                thing.background.clear();
            } else if parse_colour(value).is_some() {
                thing.background = value.to_string();
            } else {
                return Err("a colour is written #AARRGGBB (or #RRGGBB), or none".into());
            }
            done(out, format!("&e{id}&f: background &b{value}"));
        }
        "permission" => {
            thing.permission = match first {
                None | Some("none") => String::new(),
                Some(node) => node.to_string(),
            };
            out.lines.push(format!("&e{id}&f: clicking needs &b{}", if thing.permission.is_empty() { "nothing" } else { &thing.permission }));
        }
        "skin" => {
            if thing.kind != Kind::Player {
                return Err(format!("{id} is not a player NPC"));
            }
            thing.what = first.unwrap_or("").to_string();
            done(out, format!("&e{id} &fnow wears {}'s skin", if thing.what.is_empty() { "the default" } else { &thing.what }));
        }
        "type" | "item" | "block" | "particle" => {
            let name = clean_name(rest);
            let kind = match sub.as_str() {
                "type" => Kind::Mob,
                "item" => Kind::Item,
                "block" => Kind::Block,
                _ => Kind::Particle,
            };
            check_what(kind, &name, env)?;
            (thing.kind, thing.what) = (kind, name.clone());
            done(out, format!("&e{id} &fis now {} &b{name}", kind.name()));
        }
        "equip" => {
            let slot = first.unwrap_or("").to_lowercase();
            if !SLOTS.contains(&slot.as_str()) {
                return Err(format!("a slot is one of: {}", SLOTS.join(", ")));
            }
            let item = match words.next().map(clean_name) {
                None => env.held_item().ok_or("hold an item, or name one")?,
                Some(hand) if hand == "hand" => env.held_item().ok_or("you are not holding anything")?,
                Some(item) => item,
            };
            if item == "none" || item == "air" {
                thing.equipment.remove(&slot);
            } else {
                if !env.is_item(&item) {
                    return Err(format!("{item} is not an item"));
                }
                thing.equipment.insert(slot.clone(), item.clone());
            }
            done(out, format!("&e{id}&f: {slot} &b{item}"));
        }
        "action" => match first.map(str::to_lowercase).as_deref() {
            Some("add") => {
                let click = match words.next().map(str::to_lowercase).as_deref() {
                    Some("any") => Click::Any,
                    Some("left") => Click::Left,
                    Some("right") => Click::Right,
                    _ => return Err("say which click: any, left or right".into()),
                };
                let kind = words.next().unwrap_or("").to_lowercase();
                let Some((_, what)) = DO_NAMES.iter().find(|(name, _)| *name == kind) else {
                    return Err(format!("an action is one of: {}", DO_NAMES.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")));
                };
                let value = words.collect::<Vec<_>>().join(" ");
                if value.is_empty() {
                    return Err("an action needs a value: the message, command, sound or place".into());
                }
                if *what == Do::Teleport && value.split_whitespace().take(3).filter(|w| w.parse::<f64>().is_ok()).count() < 3 {
                    return Err("teleport needs x y z, and a world name if not this one".into());
                }
                thing.actions.push(Action { click, what: *what, value: value.trim_start_matches('/').to_string() });
                let (kind, count) = (thing.kind, thing.actions.len());
                done(out, format!("&e{id} &fnow has {count} action{}", if count == 1 { "" } else { "s" }));
                if click == Click::Left && kind != Kind::Mob {
                    out.lines.push("&7note: Pumpkin only reports a left click on mob NPCs, so this one will not fire; use right or any".into());
                }
            }
            Some("clear") => {
                thing.actions.clear();
                done(out, format!("&e{id} &fhas no actions now"));
            }
            Some("remove") => {
                let n: usize = number(words.next(), "the action's number")?;
                if n == 0 || n > thing.actions.len() {
                    return Err(format!("{id} has {} action{}", thing.actions.len(), if thing.actions.len() == 1 { "" } else { "s" }));
                }
                thing.actions.remove(n - 1);
                done(out, format!("&faction {n} of &e{id} &fremoved"));
            }
            Some("list") | None => {
                out.lines.push(format!("&e{id} &fhas {} action{}", thing.actions.len(), if thing.actions.len() == 1 { "" } else { "s" }));
                for (i, action) in thing.actions.iter().enumerate() {
                    out.lines.push(format!("&7{}. &f{} click &7-> &b{} &f{}", i + 1, format!("{:?}", action.click).to_lowercase(), format!("{:?}", action.what).to_lowercase(), action.value));
                }
            }
            Some(other) => return Err(format!("action add, list, remove or clear, not {other}")),
        },
        other => return Err(format!("there is no /{} {other}; /{} help lists the commands", group.command(), group.command())),
    }
    Ok(())
}

fn check_what(kind: Kind, name: &str, env: &dyn Env) -> Result<(), String> {
    match kind {
        Kind::Mob if NOT_BODIES.contains(&name) => Err(format!("{name} can't be an NPC")),
        Kind::Mob if !ENTITY_TYPES.contains(&name) => Err(format!("{name} is not a kind of mob")),
        Kind::Block if !env.is_block(name) => Err(format!("{name} is not a block")),
        Kind::Item if !env.is_item(name) => Err(format!("{name} is not an item")),
        Kind::Particle if !PARTICLES.contains(&name) => Err(format!("{name} is not a particle")),
        _ => Ok(()),
    }
}

fn create(store: &mut Store, group: Group, rest: &str, env: &dyn Env, out: &mut Outcome) -> Result<(), String> {
    let word = group.word();
    let mut parts = rest.splitn(3, char::is_whitespace);
    let id = parts.next().unwrap_or("").to_lowercase();
    let kind_word = parts.next().unwrap_or("").to_lowercase();
    let value = parts.next().unwrap_or("").trim();
    if !valid_id(&id) {
        return Err(format!("give the {word} a short name of letters, numbers, - or _: /{} create <id> ...", group.command()));
    }
    if store.get(group, &id).is_some() {
        return Err(format!("there is already a {word} called {id}"));
    }
    let (world, x, y, z, yaw, _) = env.here().ok_or("stand where it should go: only a player can create one")?;
    let mut thing = Thing { id: id.clone(), group, world, x, y, z, yaw, ..Thing::default() };
    match (group, kind_word.as_str()) {
        (_, "") => return Err(format!("say what kind: /{} help shows them", group.command())),
        (Group::Npc, "player") => {
            thing.kind = Kind::Player;
            thing.what = value.split_whitespace().next().unwrap_or("").to_string();
            thing.lines = vec![if thing.what.is_empty() { id.clone() } else { thing.what.clone() }];
            thing.look = true;
        }
        (_, "block" | "item") => {
            thing.kind = if kind_word == "block" { Kind::Block } else { Kind::Item };
            thing.what = clean_name(value);
            if thing.what.is_empty() {
                thing.what = env.held_item().ok_or(format!("name the {kind_word}, or hold it"))?;
            }
            check_what(thing.kind, &thing.what, env)?;
            if group == Group::Display && thing.kind == Kind::Item {
                thing.spin = 45.0;
                thing.billboard = "fixed".into();
            } else if thing.kind == Kind::Block {
                thing.billboard = "fixed".into();
            }
        }
        (Group::Display, "text") => {
            thing.kind = Kind::Text;
            if value.is_empty() {
                return Err("give the text: | starts a new line".into());
            }
            thing.lines = lines_of(value);
        }
        (Group::Display, "particle") => {
            thing.kind = Kind::Particle;
            thing.what = clean_name(value);
            check_what(Kind::Particle, &thing.what, env)?;
        }
        (Group::Npc, mob) => {
            thing.kind = Kind::Mob;
            thing.what = clean_name(&format!("{mob} {value}"));
            check_what(Kind::Mob, &thing.what, env)?;
            thing.look = true;
        }
        (Group::Display, other) => return Err(format!("a display is text, item, block or particle, not {other}")),
    }
    // snap blocks to the grid so they line up with the world
    if thing.kind == Kind::Block {
        (thing.x, thing.y, thing.z, thing.yaw) = (thing.x.floor(), thing.y.floor(), thing.z.floor(), 0.0);
    }
    out.changed.push(key(group, &id));
    out.lines.push(format!("&fcreated {word} &e{}", thing.summary()));
    store.things.push(thing);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Me(bool);
    impl Env for Me {
        fn here(&self) -> Option<(String, f64, f64, f64, f32, f32)> {
            self.0.then(|| ("overworld".to_string(), 10.5, 64.0, -3.5, 90.0, 0.0))
        }
        fn held_item(&self) -> Option<String> {
            Some("diamond_sword".into())
        }
        fn is_item(&self, id: &str) -> bool {
            ["diamond", "diamond_sword", "iron_helmet", "chest"].contains(&id)
        }
        fn is_block(&self, id: &str) -> bool {
            ["chest", "stone", "beacon"].contains(&id)
        }
    }

    fn ok(store: &mut Store, group: Group, input: &str) -> Outcome {
        let out = run(store, group, input, &Me(true));
        assert!(!out.lines.iter().any(|l| l.starts_with("&c")), "{input}: {:?}", out.lines);
        out
    }
    fn bad(store: &mut Store, group: Group, input: &str) -> String {
        let before = store.clone();
        let out = run(store, group, input, &Me(true));
        assert!(out.lines.len() == 1 && out.lines[0].starts_with("&c"), "{input}: {:?}", out.lines);
        assert_eq!(*store, before, "{input} changed something");
        assert!(out.changed.is_empty() && out.removed.is_empty());
        out.lines[0].clone()
    }

    #[test]
    fn creates_every_kind_of_npc() {
        let mut s = Store::default();
        ok(&mut s, Group::Npc, "create guide villager");
        ok(&mut s, Group::Npc, "create bob player Notch");
        ok(&mut s, Group::Npc, "create shop block Chest");
        ok(&mut s, Group::Npc, "create gem item minecraft:diamond");
        ok(&mut s, Group::Npc, "create held item");
        ok(&mut s, Group::Npc, "create golem iron golem");
        let kinds: Vec<(Kind, &str)> = s.things.iter().map(|t| (t.kind, t.what.as_str())).collect();
        assert_eq!(kinds, [(Kind::Mob, "villager"), (Kind::Player, "Notch"), (Kind::Block, "chest"), (Kind::Item, "diamond"), (Kind::Item, "diamond_sword"), (Kind::Mob, "iron_golem")]);
        assert!(s.things[0].look && s.things[1].look && !s.things[2].look);
        assert_eq!(s.things[1].lines, ["Notch"]);
        // blocks sit on the grid
        assert_eq!((s.things[2].x, s.things[2].z, s.things[2].yaw), (10.0, -4.0, 0.0));
        assert_eq!((s.things[0].x, s.things[0].yaw), (10.5, 90.0));
        assert!(bad(&mut s, Group::Npc, "create guide zombie").contains("already"));
        assert!(bad(&mut s, Group::Npc, "create x dragonfly").contains("not a kind of mob"));
        assert!(bad(&mut s, Group::Npc, "create x text_display").contains("can't be an NPC"));
        assert!(bad(&mut s, Group::Npc, "create x block diamond").contains("not a block"));
        assert!(bad(&mut s, Group::Npc, "create Bad!Name pig").contains("short name"));
        let out = run(&mut s, Group::Npc, "create y pig", &Me(false));
        assert!(out.lines[0].contains("only a player"));
    }

    #[test]
    fn creates_every_kind_of_display() {
        let mut s = Store::default();
        ok(&mut s, Group::Display, "create welcome text &6Welcome|&7to the server");
        ok(&mut s, Group::Display, "create prize item diamond");
        ok(&mut s, Group::Display, "create light block beacon");
        ok(&mut s, Group::Display, "create sparks particle flame");
        assert_eq!(s.things[0].lines, ["&6Welcome", "&7to the server"]);
        assert_eq!(s.things[1].spin, 45.0);
        assert_eq!(s.things[3].kind, Kind::Particle);
        assert!(bad(&mut s, Group::Display, "create x particle glitter").contains("not a particle"));
        assert!(bad(&mut s, Group::Display, "create x villager").contains("text, item, block or particle"));
        // NPCs and displays have their own names
        ok(&mut s, Group::Npc, "create welcome pig");
        assert_eq!(s.things.len(), 5);
        let list = ok(&mut s, Group::Display, "list");
        assert_eq!(list.lines.len(), 5);
    }

    #[test]
    fn edits_text_look_and_gear() {
        let mut s = Store::default();
        ok(&mut s, Group::Npc, "create guide villager");
        let out = ok(&mut s, Group::Npc, "text guide &aGuide | &7Click me");
        assert_eq!(out.changed, ["npc:guide"]);
        ok(&mut s, Group::Npc, "addline guide third");
        ok(&mut s, Group::Npc, "setline guide 2 &7Right-click me");
        ok(&mut s, Group::Npc, "removeline guide 3");
        assert_eq!(s.things[0].lines, ["&aGuide", "&7Right-click me"]);
        assert!(bad(&mut s, Group::Npc, "setline guide 9 x").contains("has 2 lines"));
        ok(&mut s, Group::Npc, "look guide off");
        ok(&mut s, Group::Npc, "glow guide");
        ok(&mut s, Group::Npc, "scale guide 1.5");
        ok(&mut s, Group::Npc, "equip guide head iron_helmet");
        ok(&mut s, Group::Npc, "equip guide mainhand hand");
        ok(&mut s, Group::Npc, "rotate guide 180 10");
        let t = &s.things[0];
        assert!(!t.look && t.glow && t.scale == 1.5 && t.yaw == 180.0 && t.pitch == 10.0);
        assert_eq!(t.equipment.get("head").map(String::as_str), Some("iron_helmet"));
        assert_eq!(t.equipment.get("mainhand").map(String::as_str), Some("diamond_sword"));
        ok(&mut s, Group::Npc, "equip guide head none");
        assert!(!s.things[0].equipment.contains_key("head"));
        assert!(bad(&mut s, Group::Npc, "equip guide hat diamond").contains("slot"));
        assert!(bad(&mut s, Group::Npc, "equip guide head pumpkin_pie").contains("not an item"));
        assert!(bad(&mut s, Group::Npc, "scale guide 99").contains("between"));
        assert!(bad(&mut s, Group::Npc, "skin guide Notch").contains("not a player"));
        assert!(bad(&mut s, Group::Npc, "wobble guide").contains("help"));
        assert!(bad(&mut s, Group::Npc, "glow nobody").contains("no NPC called nobody"));
    }

    #[test]
    fn actions_are_added_listed_and_removed() {
        let mut s = Store::default();
        ok(&mut s, Group::Npc, "create guide villager");
        ok(&mut s, Group::Npc, "action guide add right message &aHello, %player%!");
        ok(&mut s, Group::Npc, "action guide add left console /give %player% diamond 1");
        ok(&mut s, Group::Npc, "action guide add any teleport 0 80 0 overworld");
        assert_eq!(s.things[0].actions[1], Action { click: Click::Left, what: Do::Console, value: "give %player% diamond 1".into() });
        assert_eq!(ok(&mut s, Group::Npc, "action guide list").lines.len(), 4);
        assert!(bad(&mut s, Group::Npc, "action guide add up message x").contains("which click"));
        assert!(bad(&mut s, Group::Npc, "action guide add any dance x").contains("one of"));
        assert!(bad(&mut s, Group::Npc, "action guide add any message").contains("needs a value"));
        assert!(bad(&mut s, Group::Npc, "action guide add any teleport spawn").contains("x y z"));
        ok(&mut s, Group::Npc, "action guide remove 1");
        assert_eq!(s.things[0].actions.len(), 2);
        // a left click can only be noticed on a mob
        ok(&mut s, Group::Npc, "create box block chest");
        let note = ok(&mut s, Group::Npc, "action box add left message hi");
        assert!(note.lines.len() == 2 && note.lines[1].contains("left click"), "{:?}", note.lines);
        assert_eq!(ok(&mut s, Group::Npc, "action guide add left message hi").lines.len(), 1);
        ok(&mut s, Group::Npc, "action guide remove 3");
        ok(&mut s, Group::Npc, "cooldown guide 3");
        ok(&mut s, Group::Npc, "permission guide shop.use");
        assert_eq!((s.things[0].cooldown, s.things[0].permission.as_str()), (3.0, "shop.use"));
        ok(&mut s, Group::Npc, "action guide clear");
        assert!(s.things[0].actions.is_empty());
    }

    #[test]
    fn display_settings_move_remove_and_file() {
        let mut s = Store::default();
        ok(&mut s, Group::Display, "create sign text Hello");
        for line in ["billboard sign vertical", "background sign #80000000", "shadow sign on", "seethrough sign", "width sign 120", "align sign left", "spin sign 30", "range sign 2", "particle sign heart", "count sign 12", "interval sign 4", "spread sign 0.5", "speed sign 0.1"] {
            ok(&mut s, Group::Display, line);
        }
        let t = &s.things[0];
        assert_eq!((t.billboard.as_str(), t.background.as_str(), t.shadow, t.see_through, t.line_width, t.alignment.as_str()), ("vertical", "#80000000", true, true, 120, "left"));
        assert_eq!((t.kind, t.what.as_str(), t.count, t.interval), (Kind::Particle, "heart", 12, 4));
        assert_eq!(parse_colour("#80000000"), Some(0x8000_0000u32 as i32));
        assert_eq!(parse_colour("#ff0000"), Some(0xFFFF_0000u32 as i32));
        assert!(bad(&mut s, Group::Display, "billboard sign sideways").contains("one of"));
        assert!(bad(&mut s, Group::Display, "background sign red").contains("#AARRGGBB"));
        let tp = ok(&mut s, Group::Display, "tp sign");
        assert_eq!(tp.teleport, Some(("overworld".to_string(), 10.5, 64.0, -3.5)));
        // the file keeps everything, and an older file with fewer settings still loads
        let again = Store::from_json(&s.to_json()).unwrap();
        assert_eq!(again, s);
        let old = Store::from_json(r#"{"things":[{"id":"a","group":"npc","kind":"mob","what":"pig","world":"overworld","x":1,"y":2,"z":3}]}"#).unwrap();
        assert_eq!((old.things[0].scale, old.things[0].look_range), (1.0, 8.0));
        let out = ok(&mut s, Group::Display, "remove sign");
        assert_eq!(out.removed, ["display:sign"]);
        assert!(s.things.is_empty());
        assert!(ok(&mut s, Group::Display, "help").lines.len() > 10);
        assert!(ok(&mut s, Group::Npc, "").lines.len() > 10);
        assert!(ok(&mut s, Group::Npc, "reload").reload);
    }
}
