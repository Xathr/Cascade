//! What an NPC or display is, and the file they are saved in.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A player-shaped figure; `what` is the name of the player whose skin it wears.
    Player,
    /// Any mob or other entity; `what` is its type, such as `villager`.
    Mob,
    /// A block; `what` is the block's name.
    Block,
    /// An item; `what` is the item's name.
    Item,
    /// Floating text; the text is in `lines`.
    Text,
    /// A stream of particles; `what` is the particle's name.
    Particle,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Player => "player",
            Kind::Mob => "mob",
            Kind::Block => "block",
            Kind::Item => "item",
            Kind::Text => "text",
            Kind::Particle => "particle",
        }
    }
}

/// Which list a thing is shown in: `/npc` or `/display`. They are built the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Group {
    Npc,
    Display,
}

impl Group {
    pub fn word(self) -> &'static str {
        match self {
            Group::Npc => "NPC",
            Group::Display => "display",
        }
    }
    pub fn command(self) -> &'static str {
        match self {
            Group::Npc => "npc",
            Group::Display => "display",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Click {
    Any,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Do {
    /// Send the player a chat message.
    Message,
    /// Send everyone a chat message.
    Broadcast,
    /// Run a command as the console.
    Console,
    /// Run a command as the player who clicked.
    Player,
    /// Play a sound to the player.
    Sound,
    /// Move the player: `x y z` or `x y z world`.
    Teleport,
    /// Show the player a title; `title|subtitle`.
    Title,
    /// Show the player text above the hotbar.
    Actionbar,
}

pub const DO_NAMES: &[(&str, Do)] = &[
    ("message", Do::Message),
    ("broadcast", Do::Broadcast),
    ("console", Do::Console),
    ("player", Do::Player),
    ("sound", Do::Sound),
    ("teleport", Do::Teleport),
    ("title", Do::Title),
    ("actionbar", Do::Actionbar),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub click: Click,
    #[serde(rename = "do")]
    pub what: Do,
    /// The message, command, sound or place. `%player%` is the player who clicked and `%id%`
    /// the thing clicked.
    pub value: String,
}

pub const SLOTS: &[&str] = &["mainhand", "offhand", "head", "chest", "legs", "feet"];
pub const BILLBOARDS: &[&str] = &["center", "fixed", "vertical", "horizontal"];
pub const ALIGNMENTS: &[&str] = &["center", "left", "right"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Thing {
    pub id: String,
    pub group: Group,
    pub kind: Kind,
    pub what: String,
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    /// The name above an NPC, or the text of a text display. `&` colour codes.
    pub lines: Vec<String>,
    /// Turn to face the nearest player.
    pub look: bool,
    /// How near a player must be to be looked at, in blocks.
    pub look_range: f64,
    pub glow: bool,
    pub scale: f32,
    pub baby: bool,
    pub sneaking: bool,
    pub invisible: bool,
    /// Item names by slot (see `SLOTS`).
    pub equipment: BTreeMap<String, String>,
    pub actions: Vec<Action>,
    /// Seconds a player must wait between clicks.
    pub cooldown: f32,
    /// A permission a player needs to click; empty for none.
    pub permission: String,
    // displays
    pub billboard: String,
    /// The colour behind text as `#AARRGGBB`; empty for the game's default.
    pub background: String,
    pub shadow: bool,
    pub see_through: bool,
    pub line_width: i32,
    pub alignment: String,
    /// Degrees turned every second.
    pub spin: f32,
    /// How far away the display can be seen, as the game counts it (1 is normal).
    pub view_range: f32,
    // particles
    pub count: i32,
    /// Ticks between bursts of particles.
    pub interval: u32,
    pub spread: f32,
    pub speed: f32,
    /// For a display: the id of the NPC it is attached to, or empty. An attached display is
    /// placed at the NPC plus `offset`, and goes where the NPC goes.
    pub attached: String,
    pub offset: [f64; 3],
}

impl Default for Thing {
    fn default() -> Self {
        Self {
            id: String::new(),
            group: Group::Npc,
            kind: Kind::Mob,
            what: String::new(),
            world: String::new(),
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            lines: Vec::new(),
            look: false,
            look_range: 8.0,
            glow: false,
            scale: 1.0,
            baby: false,
            sneaking: false,
            invisible: false,
            equipment: BTreeMap::new(),
            actions: Vec::new(),
            cooldown: 0.5,
            permission: String::new(),
            billboard: "center".into(),
            background: String::new(),
            shadow: false,
            see_through: false,
            line_width: 200,
            alignment: "center".into(),
            spin: 0.0,
            view_range: 1.0,
            count: 5,
            interval: 10,
            spread: 0.3,
            speed: 0.0,
            attached: String::new(),
            offset: [0.0; 3],
        }
    }
}

impl Thing {
    /// Whether this is drawn with a display entity (so display settings apply to its body).
    pub fn is_display_body(&self) -> bool {
        matches!(self.kind, Kind::Block | Kind::Item | Kind::Text)
    }

    /// How high above its feet a display attached to this sits by default.
    pub fn label_height(&self) -> f64 {
        let base = match self.kind {
            Kind::Player => 2.05,
            Kind::Mob => match self.what.as_str() {
                "villager" | "zombie" | "skeleton" | "husk" | "drowned" | "piglin" | "wandering_trader" | "witch" | "pillager" | "stray" => 2.2,
                "enderman" => 3.1,
                "iron_golem" | "warden" => 3.0,
                "chicken" | "cat" | "rabbit" | "fox" | "bat" | "parrot" | "silverfish" | "bee" => 0.9,
                "pig" | "sheep" | "wolf" | "slime" | "armadillo" => 1.2,
                _ => 1.9,
            },
            Kind::Block => 1.25,
            Kind::Item => 0.75,
            Kind::Text | Kind::Particle => 0.0,
        };
        base * self.scale.max(0.05) as f64
    }

    pub fn summary(&self) -> String {
        let what = if self.what.is_empty() { String::new() } else { format!(" {}", self.what) };
        if self.attached.is_empty() {
            format!("{} ({}{}) at {:.1} {:.1} {:.1} in {}", self.id, self.kind.name(), what, self.x, self.y, self.z, self.world)
        } else {
            format!("{} ({}{}) attached to {}, {:+.2} {:+.2} {:+.2} from it", self.id, self.kind.name(), what, self.attached, self.offset[0], self.offset[1], self.offset[2])
        }
    }
}

/// `#AARRGGBB` or `#RRGGBB` as the number the game uses.
pub fn parse_colour(text: &str) -> Option<i32> {
    let hex = text.trim().trim_start_matches('#');
    match hex.len() {
        6 => u32::from_str_radix(hex, 16).ok().map(|rgb| (0xFF00_0000 | rgb) as i32),
        8 => u32::from_str_radix(hex, 16).ok().map(|argb| argb as i32),
        _ => None,
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Store {
    pub things: Vec<Thing>,
}

impl Store {
    pub fn get(&self, group: Group, id: &str) -> Option<&Thing> {
        self.things.iter().find(|t| t.group == group && t.id == id)
    }
    pub fn get_mut(&mut self, group: Group, id: &str) -> Option<&mut Thing> {
        self.things.iter_mut().find(|t| t.group == group && t.id == id)
    }
    pub fn remove(&mut self, group: Group, id: &str) -> bool {
        let before = self.things.len();
        self.things.retain(|t| !(t.group == group && t.id == id));
        self.things.len() != before
    }
    /// `thing` as it stands in the world: an attached display is at its NPC plus its offset.
    pub fn placed(&self, thing: &Thing) -> Thing {
        let mut placed = thing.clone();
        if let Some(npc) = (!thing.attached.is_empty()).then(|| self.get(Group::Npc, &thing.attached)).flatten() {
            placed.world = npc.world.clone();
            (placed.x, placed.y, placed.z) = (npc.x + thing.offset[0], npc.y + thing.offset[1], npc.z + thing.offset[2]);
        }
        placed
    }

    /// Every thing as it stands in the world.
    pub fn all_placed(&self) -> Vec<Thing> {
        self.things.iter().map(|t| self.placed(t)).collect()
    }

    /// NPCs used to carry their own name. Each such name becomes a text display attached to the
    /// NPC, called `<npc>-name`. Returns how many were made.
    pub fn names_to_displays(&mut self) -> usize {
        let mut made = Vec::new();
        for npc in self.things.iter_mut().filter(|t| t.group == Group::Npc && !t.lines.is_empty()) {
            let lines = std::mem::take(&mut npc.lines);
            let height = npc.label_height() + 0.1;
            let at_middle = if npc.kind == Kind::Block { 0.5 * npc.scale as f64 } else { 0.0 };
            made.push(Thing { id: format!("{}-name", npc.id), group: Group::Display, kind: Kind::Text, lines, attached: npc.id.clone(), offset: [at_middle, height, at_middle], world: npc.world.clone(), x: npc.x, y: npc.y, z: npc.z, ..Thing::default() });
        }
        let count = made.len();
        for display in made {
            if self.get(Group::Display, &display.id).is_none() {
                self.things.push(display);
            }
        }
        count
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }
}

/// The key a thing's live entities are kept under.
pub fn key(group: Group, id: &str) -> String {
    format!("{}:{id}", group.command())
}
