//! The plugin: builds NPCs and displays in the world out of the saved list, keeps them facing
//! players, turning and sparkling, and carries out their actions when clicked.

use crate::color;
use crate::commands::{self, Env};
use crate::model::*;
use crate::names;
use pumpkin_plugin_api::command::{Command, CommandError, CommandNode, CommandSender, ConsumedArgs};
use pumpkin_plugin_api::command_wit::{Arg, ArgumentType, StringType};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::common::{Hand, NbtTag, NbtTree};
use pumpkin_plugin_api::display::EntityDisplayExt;
use pumpkin_plugin_api::display_wit::{BillboardMode, TextAlignment, Vector3f};
use pumpkin_plugin_api::events::*;
use pumpkin_plugin_api::events_wit::EntityInteractionAction;
use pumpkin_plugin_api::permission::{Permission, PermissionDefault, PermissionLevel};
use pumpkin_plugin_api::scheduler::SchedulerExt;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::world::{Entity, EquipmentSlot, World};
use pumpkin_plugin_api::{BlockTypeExt, Context, EntityType, ItemStack, Player, Plugin, PluginMetadata, Server, permissions, register_plugin};
use std::cell::RefCell;
use std::collections::HashMap;

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// A thing is built when a player comes this near, and taken away when all are further than
/// `KEEP_WITHIN`. This keeps far-off chunks from being touched.
const BUILD_WITHIN: f64 = 48.0;
const KEEP_WITHIN: f64 = 64.0;
const TAG: (&str, &str) = ("cascade", "thing");
/// Mobs the sun sets alight.
const BURNS_IN_DAYLIGHT: &[&str] = &["zombie", "zombie_villager", "drowned", "skeleton", "stray", "bogged", "parched", "phantom", "zombie_horse", "skeleton_horse"];

/// What a thing is made of while it is in the world.
struct Live {
    body: Option<Entity>,
    hit: Option<Entity>,
    /// The entity numbers a click can arrive on.
    ids: Vec<i32>,
    yaw: f32,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::default());
    static LIVE: RefCell<HashMap<String, Live>> = RefCell::new(HashMap::new());
    static FOLDER: RefCell<String> = const { RefCell::new(String::new()) };
    static TICK: RefCell<u64> = const { RefCell::new(0) };
    /// When each player last clicked each thing: (thing, player) -> tick.
    static CLICKS: RefCell<HashMap<(String, String), u64>> = RefCell::new(HashMap::new());
    /// Who has been sent each player NPC's skin: thing -> (its entity number, players).
    static SKINNED: RefCell<HashMap<String, (i32, Vec<String>)>> = RefCell::new(HashMap::new());
    /// The web editor's server, when it is on.
    static WEB: RefCell<Option<crate::web::Web>> = const { RefCell::new(None) };
    /// The host to put in the editor's link, from config.toml; empty to work it out.
    static WEB_ADDRESS: RefCell<String> = const { RefCell::new(String::new()) };
}

fn tagged(msg: &str) -> String {
    format!("{} {msg}", color::tag("[Cascade]"))
}
fn info(msg: &str) {
    pumpkin_plugin_api::logging::log(pumpkin_plugin_api::logging::LogLevel::Info, &tagged(msg));
}
fn warn(msg: &str) {
    pumpkin_plugin_api::logging::log(pumpkin_plugin_api::logging::LogLevel::Warn, &tagged(&color::warn(msg)));
}

/// Shows the sender a line written with `&` codes and buttons: console colours in the console,
/// the same colours and clickable buttons in game. The plugin's tag goes in front when `tag` is
/// set, as on the first line of a reply.
fn say(sender: &CommandSender, line: &str, tag: bool) {
    if sender.is_console() {
        let text = color::console(line);
        sender.send_message(TextComponent::text(&if tag { tagged(&text) } else { format!("{} {text}", color::tag("[Cascade]")) }));
    } else {
        match TextComponent::from_json(&color::chat(line, tag.then_some("[Cascade]"))) {
            Ok(message) => sender.send_message(message),
            Err(_) => sender.send_message(TextComponent::text(&color::strip(&color::with_colours(|| color::console(line))))),
        }
    }
}

/// Text the server's owner wrote, with `&` colour codes, as the game shows it.
fn component(text: &str) -> TextComponent {
    TextComponent::from_legacy_string_with_code(text, '&')
}

fn strip_ns(name: &str) -> String {
    name.strip_prefix("minecraft:").unwrap_or(name).to_string()
}

fn is_item(id: &str) -> bool {
    id != "air" && strip_ns(&ItemStack::new(id, 1).get_registry_key()) == id
}

fn world_named(server: &Server, name: &str) -> Option<World> {
    let wanted = strip_ns(name);
    server.get_all_worlds().into_iter().find(|w| strip_ns(&w.get_id()).eq_ignore_ascii_case(&wanted))
}

fn tag(entity: &Entity, key: &str) {
    entity.set_custom_data(TAG.0, TAG.1, &NbtTree { root: 0, tags: vec![NbtTag::StringTag(key.to_string())] });
    entity.set_invulnerable(true);
    entity.set_silent(true);
    entity.set_has_gravity(false);
}

fn slot_of(name: &str) -> Option<EquipmentSlot> {
    Some(match name {
        "mainhand" => EquipmentSlot::MainHand,
        "offhand" => EquipmentSlot::OffHand,
        "head" => EquipmentSlot::Head,
        "chest" => EquipmentSlot::Chest,
        "legs" => EquipmentSlot::Legs,
        "feet" => EquipmentSlot::Feet,
        _ => return None,
    })
}

/// A text display at `pos` showing `lines`.
fn text_display(world: &World, pos: (f64, f64, f64), lines: &[String], thing: &Thing, own_settings: bool) -> Entity {
    let entity = world.spawn_entity(EntityType::TextDisplay, pos);
    if let Some(text) = entity.as_text_display() {
        text.set_text(component(&lines.join("\n")));
        if own_settings {
            text.set_shadow(thing.shadow);
            text.set_see_through(thing.see_through);
            text.set_line_width(thing.line_width);
            text.set_alignment(match thing.alignment.as_str() {
                "left" => TextAlignment::Left,
                "right" => TextAlignment::Right,
                _ => TextAlignment::Center,
            });
            if let Some(colour) = parse_colour(&thing.background) {
                text.set_default_background(false);
                text.set_background(colour);
            }
        }
    }
    entity
}

fn display_settings(entity: &Entity, thing: &Thing, billboard: &str, scale: f32) {
    let Some(display) = entity.as_display() else { return };
    display.set_billboard(match billboard {
        "fixed" => BillboardMode::Fixed,
        "vertical" => BillboardMode::Vertical,
        "horizontal" => BillboardMode::Horizontal,
        _ => BillboardMode::Center,
    });
    display.set_view_range(thing.view_range);
    let mut shape = display.get_transformation();
    shape.scale = Vector3f { x: scale, y: scale, z: scale };
    display.set_transformation(shape);
    // lets a turning display turn smoothly between updates
    display.set_teleport_duration(3);
}

/// Puts a thing into the world.
fn build(server: &Server, thing: &Thing) -> Option<Live> {
    let world = world_named(server, &thing.world)?;
    let key = key(thing.group, &thing.id);
    let pos = (thing.x, thing.y, thing.z);
    let scale = thing.scale.max(0.05);
    let mut live = Live { body: None, hit: None, ids: Vec::new(), yaw: thing.yaw };
    // what a click lands on when the body itself can't be clicked
    let mut hit_box: Option<((f64, f64, f64), f32, f32)> = None;
    match thing.kind {
        Kind::Player | Kind::Mob => {
            let kind = if thing.kind == Kind::Player { EntityType::Mannequin } else { names::entity_type(&thing.what)? };
            let entity = world.spawn_entity(kind, pos);
            tag(&entity, &key);
            if let Some(mob) = entity.as_mob() {
                mob.clear_ai_goals();
                mob.set_ai_disabled(true);
            }
            if let Some(living) = entity.as_living() {
                if (scale - 1.0).abs() > 0.001 {
                    living.set_attribute_base(pumpkin_plugin_api::Attribute::Scale, scale as f64);
                }
                if thing.baby {
                    living.set_age(-24000);
                }
                for (slot, item) in &thing.equipment {
                    if let Some(slot) = slot_of(slot) {
                        living.set_equipment(slot, Some(ItemStack::new(item, 1)));
                    }
                }
                // Something on the head is what keeps these mobs from catching fire in daylight.
                // With no head gear chosen, a stone button does it: the smallest thing there is.
                if thing.kind == Kind::Mob && BURNS_IN_DAYLIGHT.contains(&thing.what.as_str()) && !thing.equipment.contains_key("head") {
                    living.set_equipment(EquipmentSlot::Head, Some(ItemStack::new("stone_button", 1)));
                }
            }
            entity.set_rotation(thing.yaw, thing.pitch);
            entity.set_sneaking(thing.sneaking);
            entity.set_invisible(thing.invisible);
            entity.set_glowing(thing.glow);
            live.body = Some(entity);
        }
        Kind::Block => {
            let entity = world.spawn_entity(EntityType::BlockDisplay, pos);
            tag(&entity, &key);
            if let (Some(display), Some(block)) = (entity.as_block_display(), pumpkin_plugin_api::world::Block::of(thing.what.as_str())) {
                display.set_block_state_id(block.default_state_id);
            }
            display_settings(&entity, thing, &thing.billboard, scale);
            entity.set_rotation(thing.yaw, thing.pitch);
            entity.set_glowing(thing.glow);
            hit_box = Some(((thing.x + 0.5 * scale as f64, thing.y, thing.z + 0.5 * scale as f64), scale, scale));
            live.body = Some(entity);
        }
        Kind::Item => {
            let centre = (thing.x, thing.y + 0.35 * scale as f64, thing.z);
            let entity = world.spawn_entity(EntityType::ItemDisplay, centre);
            tag(&entity, &key);
            if let Some(display) = entity.as_item_display() {
                display.set_item(Some(ItemStack::new(&thing.what, 1)));
            }
            display_settings(&entity, thing, &thing.billboard, scale);
            entity.set_rotation(thing.yaw, thing.pitch);
            entity.set_glowing(thing.glow);
            hit_box = Some((pos, 0.7 * scale, 0.7 * scale));
            live.body = Some(entity);
        }
        Kind::Text => {
            let entity = text_display(&world, pos, &thing.lines, thing, true);
            tag(&entity, &key);
            display_settings(&entity, thing, &thing.billboard, scale);
            entity.set_rotation(thing.yaw, thing.pitch);
            entity.set_glowing(thing.glow);
            let height = 0.28 * thing.lines.len().max(1) as f32 * scale;
            hit_box = Some((pos, 1.6 * scale, height));
            live.body = Some(entity);
        }
        Kind::Particle => {}
    }
    if let (Some(shape), false) = (hit_box, thing.actions.is_empty()) {
        live.hit = Some(click_box(&world, shape, &key));
    }
    live.ids = [&live.body, &live.hit].into_iter().flatten().map(|e| e.get_id() as i32).collect();
    Some(live)
}

/// An unseen box that takes clicks for a thing that can't be clicked itself.
fn click_box(world: &World, (at, width, height): ((f64, f64, f64), f32, f32), key: &str) -> Entity {
    let entity = world.spawn_entity(EntityType::Interaction, at);
    tag(&entity, key);
    if let Some(interaction) = entity.as_interaction() {
        interaction.set_width(width);
        interaction.set_height(height);
        interaction.set_response(true);
    }
    entity
}

/// The entity data that gives a mannequin the skin of the player called `name`: the game
/// looks the skin up itself from the name.
fn skin_data(name: &str) -> Vec<u8> {
    let mut data = vec![17, 41, 0, 1, name.len() as u8];
    data.extend_from_slice(name.as_bytes());
    // no id, no properties, no body, cape or elytra texture, no model; then the end mark
    data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0xFF]);
    data
}

/// Pumpkin does not know about mannequin skins, so each player who comes near a player NPC is
/// sent its skin once, and again if they go away and come back.
fn skin_pass(things: &[Thing], near: &[(String, f64, f64, f64, Player)]) {
    for thing in things.iter().filter(|t| t.kind == Kind::Player && !t.what.is_empty() && t.what.len() <= 16) {
        let key = key(thing.group, &thing.id);
        let Some(id) = LIVE.with(|l| l.borrow().get(&key).and_then(|live| live.body.as_ref().map(|b| b.get_id() as i32))) else {
            SKINNED.with(|s| s.borrow_mut().remove(&key));
            continue;
        };
        let mut seen: Vec<String> = Vec::new();
        for at in near {
            if !distance2(thing, at).is_some_and(|d| d <= KEEP_WITHIN * KEEP_WITHIN) {
                continue;
            }
            let who = pumpkin_plugin_api::uuid::to_string(at.4.get_id());
            let known = SKINNED.with(|s| s.borrow().get(&key).is_some_and(|(body, players)| *body == id && players.contains(&who)));
            if !known {
                if let Some(java) = at.4.as_java() {
                    use pumpkin_plugin_api::java_packets::{CSetEntityMetadata, ClientboundPacket};
                    java.send_packet(&ClientboundPacket::CSetEntityMetadata(CSetEntityMetadata { entity_id: id, metadata: skin_data(&thing.what) }));
                }
            }
            seen.push(who);
        }
        SKINNED.with(|s| s.borrow_mut().insert(key, (id, seen)));
    }
}

fn unbuild(live: Live) {
    for entity in [live.body, live.hit].into_iter().flatten() {
        entity.remove();
    }
}

fn unbuild_key(key: &str) {
    if let Some(live) = LIVE.with(|l| l.borrow_mut().remove(key)) {
        unbuild(live);
    }
}

fn unbuild_all() {
    let all: Vec<Live> = LIVE.with(|l| l.borrow_mut().drain().map(|(_, v)| v).collect());
    all.into_iter().for_each(unbuild);
}

/// Where every player is: (world, x, y, z, player).
fn players(server: &Server) -> Vec<(String, f64, f64, f64, Player)> {
    server.get_all_players().into_iter().map(|p| {
        let (x, y, z) = p.get_position();
        (strip_ns(&p.get_world().get_id()), x, y, z, p)
    }).collect()
}

fn distance2(thing: &Thing, at: &(String, f64, f64, f64, Player)) -> Option<f64> {
    at.0.eq_ignore_ascii_case(&strip_ns(&thing.world)).then(|| (thing.x - at.1).powi(2) + (thing.y - at.2).powi(2) + (thing.z - at.3).powi(2))
}

/// Builds things players have come near, and takes away those everyone has left.
fn build_pass(server: &Server, near: &[(String, f64, f64, f64, Player)]) {
    let things: Vec<Thing> = STORE.with(|s| s.borrow().all_placed());
    for thing in &things {
        let key = key(thing.group, &thing.id);
        let closest = near.iter().filter_map(|p| distance2(thing, p)).fold(f64::INFINITY, f64::min);
        let built = LIVE.with(|l| l.borrow().contains_key(&key));
        if !built && closest <= BUILD_WITHIN * BUILD_WITHIN {
            match build(server, thing) {
                Some(live) => {
                    LIVE.with(|l| l.borrow_mut().insert(key, live));
                }
                None => warn(&format!("could not build {}: is the world {} loaded, and is {} right?", thing.id, thing.world, thing.what)),
            }
        } else if built && closest > KEEP_WITHIN * KEEP_WITHIN {
            unbuild_key(&key);
        }
    }
    // things deleted from the list
    let gone: Vec<String> = LIVE.with(|l| l.borrow().keys().filter(|k| !things.iter().any(|t| key(t.group, &t.id) == **k)).cloned().collect());
    gone.iter().for_each(|k| unbuild_key(k));
}

/// Removes entities this plugin made in an earlier run that are not in use now (after a crash,
/// say), so nothing is left standing twice.
fn sweep(server: &Server) {
    let mine: Vec<i32> = LIVE.with(|l| l.borrow().values().flat_map(|live| [&live.body, &live.hit].into_iter().flatten().map(|e| e.get_id() as i32).collect::<Vec<_>>()).collect());
    for world in server.get_all_worlds() {
        for entity in world.get_entities() {
            if entity.has_custom_data(TAG.0, TAG.1) && !mine.contains(&(entity.get_id() as i32)) {
                entity.remove();
            }
        }
    }
}

fn every_tick(server: Server) {
    let tick = TICK.with(|t| {
        let mut t = t.borrow_mut();
        *t += 1;
        *t
    });
    serve_web(&server);
    let things: Vec<Thing> = STORE.with(|s| s.borrow().all_placed());
    if things.is_empty() && LIVE.with(|l| l.borrow().is_empty()) {
        return;
    }
    let needs_players = tick % 10 == 0 || tick % 2 == 0;
    let near = if needs_players { players(&server) } else { Vec::new() };
    if tick % 10 == 0 {
        build_pass(&server, &near);
        skin_pass(&things, &near);
    }
    // soon after start, then every few seconds: chunks a player has just walked into may hold
    // something an earlier run left behind
    if tick == 5 || tick % 60 == 30 {
        sweep(&server);
    }

    for thing in &things {
        let key = key(thing.group, &thing.id);
        // particles
        if thing.kind == Kind::Particle && tick % thing.interval.max(1) as u64 == 0 && LIVE.with(|l| l.borrow().contains_key(&key)) {
            if let (Some(world), Some(particle)) = (world_named(&server, &thing.world), names::particle(&thing.what)) {
                let s = thing.spread as f64;
                world.spawn_particle(particle, (thing.x, thing.y, thing.z), (s, s, s), thing.speed, thing.count);
            }
        }
        // Zombies, skeletons and the like catch fire in daylight. An NPC can't be hurt, but it
        // would stand there in flames, so any fire is put out at once.
        if thing.kind == Kind::Mob {
            LIVE.with(|l| {
                if let Some(body) = l.borrow().get(&key).and_then(|live| live.body.as_ref()) {
                    if body.is_on_fire() || body.get_fire_ticks() > 0 || body.has_visual_fire() {
                        body.set_fire_ticks(0);
                        body.set_on_fire(false);
                        body.set_visual_fire(false);
                    }
                }
            });
        }
        if tick % 2 != 0 {
            continue;
        }
        LIVE.with(|l| {
            let mut live = l.borrow_mut();
            let Some(live) = live.get_mut(&key) else { return };
            let Some(body) = &live.body else { return };
            if thing.spin != 0.0 && thing.is_display_body() {
                live.yaw = (live.yaw + thing.spin / 10.0) % 360.0;
                body.set_rotation(live.yaw, thing.pitch);
            } else if thing.look {
                let range2 = thing.look_range * thing.look_range;
                let target = near.iter().filter_map(|p| distance2(thing, p).filter(|d| *d <= range2).map(|d| (d, p))).min_by(|a, b| a.0.total_cmp(&b.0));
                if let Some((_, (_, px, py, pz, _))) = target {
                    let eye = thing.y + 1.62 * thing.scale as f64 * if thing.is_display_body() { 0.0 } else { 1.0 };
                    let (dx, dy, dz) = (px - thing.x, (py + 1.62) - eye, pz - thing.z);
                    let yaw = (-dx).atan2(dz).to_degrees() as f32;
                    let pitch = (-dy).atan2((dx * dx + dz * dz).sqrt()).to_degrees() as f32;
                    if (yaw - live.yaw).abs() > 1.0 {
                        live.yaw = yaw;
                        body.set_rotation(yaw, pitch.clamp(-60.0, 60.0));
                        if let Some(mob) = body.as_mob() {
                            mob.look_at((*px, py + 1.62, *pz));
                        }
                    }
                }
            }
        });
    }
}

/// The game's lists, for checking what the web editor sends when nobody is at a keyboard.
struct Checker;
impl Env for Checker {
    fn here(&self) -> Option<(String, f64, f64, f64, f32, f32)> {
        None
    }
    fn held_item(&self) -> Option<String> {
        None
    }
    fn is_item(&self, id: &str) -> bool {
        is_item(id)
    }
    fn is_block(&self, id: &str) -> bool {
        pumpkin_plugin_api::world::Block::of(id).is_some()
    }
}

/// Answers the web editor: hands it the list, and takes a changed list back.
fn serve_web(server: &Server) {
    let mut saved = false;
    WEB.with(|w| {
        let Ok(mut web) = w.try_borrow_mut() else { return };
        let Some(web) = web.as_mut() else { return };
        web.serve(&mut |ask| match ask {
            crate::web::Ask::State => {
                let things = STORE.with(|s| serde_json::to_value(&s.borrow().things).unwrap_or_default());
                serde_json::json!({ "version": VERSION, "things": things, "mobs": commands::body_types(), "particles": names::PARTICLES, "slots": SLOTS, "actions": DO_NAMES.iter().map(|(n, _)| *n).collect::<Vec<_>>() }).to_string()
            }
            crate::web::Ask::Save(body) => {
                let checked = Store::from_json(&body).map_err(|e| format!("could not read what was sent: {e}")).and_then(|mut store| commands::validate(&mut store, &Checker).map(|()| store));
                match checked {
                    Ok(store) => {
                        let count = store.things.len();
                        STORE.with(|s| *s.borrow_mut() = store);
                        saved = true;
                        serde_json::json!({ "ok": true, "count": count }).to_string()
                    }
                    Err(error) => serde_json::json!({ "ok": false, "error": error }).to_string(),
                }
            }
        });
    });
    if saved {
        save();
        unbuild_all();
        build_pass(server, &players(server));
        info(&format!("the web editor saved {} NPCs and displays", STORE.with(|s| s.borrow().things.len())));
    }
}

/// A player clicked the entity numbered `entity_id`. Returns true if it was one of ours.
fn clicked(server: &Server, player: &Player, entity_id: i32, left: bool) -> bool {
    let Some(key) = LIVE.with(|l| l.borrow().iter().find(|(_, live)| live.ids.contains(&entity_id)).map(|(k, _)| k.clone())) else { return false };
    let Some(thing) = STORE.with(|s| s.borrow().things.iter().find(|t| crate::model::key(t.group, &t.id) == key).cloned()) else { return true };
    let who = pumpkin_plugin_api::uuid::to_string(player.get_id());
    let name = player.get_name();
    let now = TICK.with(|t| *t.borrow());
    // one click can arrive as two events, so there is always a short wait
    let wait = ((thing.cooldown * 20.0) as u64).max(4);
    let too_soon = CLICKS.with(|c| {
        let mut c = c.borrow_mut();
        let last = c.get(&(key.clone(), who.clone())).copied();
        if last.is_some_and(|at| now < at + wait) {
            true
        } else {
            c.insert((key.clone(), who.clone()), now);
            false
        }
    });
    if too_soon {
        return true;
    }
    if !thing.permission.is_empty() && !player.has_permission(&thing.permission) {
        player.send_system_message(component("&cYou can't use this."), true);
        return true;
    }
    for action in &thing.actions {
        let fits = match action.click {
            Click::Any => true,
            Click::Left => left,
            Click::Right => !left,
        };
        if !fits {
            continue;
        }
        let value = action.value.replace("%player%", &name).replace("%id%", &thing.id);
        match action.what {
            Do::Message => player.send_system_message(component(&value), false),
            Do::Actionbar => player.show_actionbar(component(&value)),
            Do::Broadcast => {
                for p in server.get_all_players() {
                    p.send_system_message(component(&value), false);
                }
            }
            Do::Title => {
                let (title, sub) = value.split_once('|').unwrap_or((&value, ""));
                if !sub.is_empty() {
                    player.show_subtitle(component(sub.trim()));
                }
                player.show_title(component(title.trim()));
            }
            Do::Sound => {
                let sound = if value.contains(':') { value.clone() } else { format!("minecraft:{value}") };
                player.play_custom_sound(&sound, pumpkin_plugin_api::player::SoundCategory::Master, 1.0, 1.0);
            }
            Do::Console => server.execute_command(&value, pumpkin_plugin_api::server::CommandSender::Console),
            Do::Player => {
                if let Some(p) = server.get_player_by_uuid(player.get_id()) {
                    server.execute_command(&value, pumpkin_plugin_api::server::CommandSender::Player(p));
                }
            }
            Do::Teleport => {
                let words: Vec<&str> = value.split_whitespace().collect();
                let n: Vec<f64> = words.iter().take(3).filter_map(|w| w.parse().ok()).collect();
                let world = words.get(3).and_then(|w| world_named(server, w)).unwrap_or_else(|| player.get_world());
                if n.len() == 3 {
                    player.teleport((n[0], n[1], n[2]), None, None, world);
                }
            }
        }
    }
    true
}

struct OnInteract;
impl EventHandler<PlayerInteractEntityEvent> for OnInteract {
    fn handle(&self, server: Server, mut e: <PlayerInteractEntityEvent as FromIntoEvent>::Data) -> <PlayerInteractEntityEvent as FromIntoEvent>::Data {
        if clicked(&server, &e.player, e.entity_id, matches!(e.action, EntityInteractionAction::Attack)) {
            e.cancelled = true;
        }
        e
    }
}

struct OnInteractUnknown;
impl EventHandler<PlayerInteractUnknownEntityEvent> for OnInteractUnknown {
    fn handle(&self, server: Server, mut e: <PlayerInteractUnknownEntityEvent as FromIntoEvent>::Data) -> <PlayerInteractUnknownEntityEvent as FromIntoEvent>::Data {
        if clicked(&server, &e.player, e.entity_id, matches!(e.action, EntityInteractionAction::Attack)) {
            e.cancelled = true;
        }
        e
    }
}

/// A left click arrives as an attack: cancel the damage and treat it as a click.
struct OnDamage;
impl EventHandler<EntityDamageByEntityEvent> for OnDamage {
    fn handle(&self, server: Server, mut e: <EntityDamageByEntityEvent as FromIntoEvent>::Data) -> <EntityDamageByEntityEvent as FromIntoEvent>::Data {
        let ours = LIVE.with(|l| l.borrow().values().any(|live| live.ids.contains(&e.entity_id)));
        if ours {
            e.cancelled = true;
            if let Some(player) = server.get_all_players().into_iter().find(|p| p.as_entity().get_id() as i32 == e.damager_id) {
                clicked(&server, &player, e.entity_id, true);
            }
        }
        e
    }
}

fn save() {
    let path = format!("{}/things.json", FOLDER.with(|f| f.borrow().clone()));
    let text = STORE.with(|s| s.borrow().to_json());
    let tmp = format!("{path}.tmp");
    if let Err(e) = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path)) {
        warn(&format!("could not save {path}: {e}"));
    }
}

fn load() -> Result<usize, String> {
    let path = format!("{}/things.json", FOLDER.with(|f| f.borrow().clone()));
    let store = match std::fs::read_to_string(&path) {
        Ok(text) => Store::from_json(&text).map_err(|e| format!("things.json has a mistake ({e}); nothing was changed"))?,
        Err(_) => Store::default(),
    };
    let mut store = store;
    // names NPCs carried in older versions become text displays attached to them
    let moved = store.names_to_displays();
    let n = store.things.len();
    STORE.with(|s| *s.borrow_mut() = store);
    if moved > 0 {
        save();
        info(&format!("{moved} NPC name{} became text display{} attached to the NPC (/display list)", if moved == 1 { "" } else { "s" }, if moved == 1 { "" } else { "s" }));
    }
    Ok(n)
}

struct Sender<'a>(&'a CommandSender);
impl Env for Sender<'_> {
    fn here(&self) -> Option<(String, f64, f64, f64, f32, f32)> {
        let p = self.0.as_player()?;
        let (x, y, z) = p.get_position();
        // half-block steps keep things lined up without fuss
        let snap = |v: f64| (v * 2.0).round() / 2.0;
        Some((strip_ns(&p.get_world().get_id()), snap(x), (y * 16.0).round() / 16.0, snap(z), (p.get_yaw() / 15.0).round() * 15.0, 0.0))
    }
    fn held_item(&self) -> Option<String> {
        let stack = self.0.as_player()?.get_item_in_hand(Hand::Right)?;
        let id = strip_ns(&stack.get_registry_key());
        (id != "air").then_some(id)
    }
    fn is_item(&self, id: &str) -> bool {
        is_item(id)
    }
    fn is_block(&self, id: &str) -> bool {
        pumpkin_plugin_api::world::Block::of(id).is_some()
    }
}

/// One word of a command as it is typed: a fixed word, or something the player fills in.
#[derive(Clone, Copy)]
enum Part {
    Word(&'static str),
    Arg(&'static str),
}

/// One branch of `/npc` or `/display`: the words that lead to it.
struct ThingCommand {
    group: Group,
    path: Vec<Part>,
}

fn text_arg(args: &ConsumedArgs, name: &str) -> String {
    match args.get_value(name) {
        Arg::Simple(text) | Arg::Msg(text) => text,
        _ => String::new(),
    }
}

impl CommandHandler for ThingCommand {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let input = self
            .path
            .iter()
            .map(|part| match part {
                Part::Word(word) => word.to_string(),
                Part::Arg(name) => text_arg(&args, name),
            })
            .collect::<Vec<_>>()
            .join(" ");
        let Some((before, out)) = STORE.with(|s| {
            let mut store = s.try_borrow_mut().ok()?;
            let before = store.clone();
            let out = commands::run(&mut store, self.group, &input, &Sender(&sender));
            Some((before != *store, out))
        }) else {
            say(&sender, "&c\u{2718} busy; try again in a moment", true);
            return Ok(0);
        };
        if before {
            save();
        }
        for key in out.removed.iter().chain(&out.changed) {
            unbuild_key(key);
        }
        if out.reload {
            match load() {
                Ok(n) => {
                    unbuild_all();
                    say(&sender, &format!("&a\u{2714} &fread things.json again: &b{n}&f NPCs and displays"), true);
                }
                Err(e) => say(&sender, &format!("&c\u{2718} {e}"), true),
            }
        }
        if !out.changed.is_empty() || out.reload {
            build_pass(&server, &players(&server));
        }
        if let (Some((world, x, y, z)), Some(player)) = (&out.teleport, sender.as_player()) {
            match world_named(&server, world) {
                Some(world) => player.teleport((*x, *y, *z), None, None, world),
                None => say(&sender, &format!("&c\u{2718} the world {world} is not loaded"), true),
            }
        }
        if out.web {
            let line = WEB.with(|w| match w.borrow().as_ref() {
                Some(web) => {
                    // the address the player connected with reaches this server, so use its host
                    let typed = sender.as_player().and_then(|p| p.as_java()).map(|j| j.get_server_address()).unwrap_or_default();
                    let host = WEB_ADDRESS.with(|a| a.borrow().clone());
                    let host = if !host.is_empty() { host } else { typed.rsplit_once(':').map_or(typed.clone(), |(h, _)| h.to_string()).trim_end_matches('.').to_string() };
                    let host = if host.is_empty() { "localhost".to_string() } else { host };
                    let url = format!("http://{host}:{}/?t={}", web.port, web.token);
                    if sender.is_console() {
                        format!("&fthe editor is at &b{url}")
                    } else {
                        format!("&fthe editor is ready: {}&7the link is secret and changes when the server restarts", commands::button("open the editor", &format!("u{url}")[1..], "Click to open it in your browser", true).replace("\u{2}r", "\u{2}u"))
                    }
                }
                None => "&c\u{2718} the web editor is off: set web_port in plugins/data/cascade/config.toml, allow the plugin network.tcp, and restart".to_string(),
            });
            say(&sender, &line, true);
        }
        out.lines.iter().enumerate().for_each(|(i, l)| say(&sender, l, i == 0));
        Ok(1)
    }
}

/// What can follow a point in a command: fixed words the game completes with Tab, or a value.
enum Spec {
    Word(&'static str, Vec<Spec>),
    /// One word of the player's own, such as an id.
    Name(&'static str, Vec<Spec>),
    /// The rest of the line.
    Rest,
}

fn words(list: &[&'static str], then: fn() -> Vec<Spec>) -> Vec<Spec> {
    list.iter().map(|w| Spec::Word(w, then())).collect()
}

fn node(group: Group, spec: &Spec, path: &[Part]) -> CommandNode {
    let (start, part, children): (CommandNode, Part, &[Spec]) = match spec {
        Spec::Word(word, children) => (CommandNode::literal(word), Part::Word(word), children),
        Spec::Name(name, children) => (CommandNode::argument(name, &ArgumentType::String(StringType::SingleWord)), Part::Arg(name), children),
        Spec::Rest => (CommandNode::argument("rest", &ArgumentType::String(StringType::Greedy)), Part::Arg("rest"), &[]),
    };
    let mut path = path.to_vec();
    path.push(part);
    let mut built = start.execute(ThingCommand { group, path: path.clone() });
    for child in children {
        built = built.then(node(group, child, &path));
    }
    built
}

/// What follows `/npc <sub> <id>`.
fn after_id(sub: &str) -> Vec<Spec> {
    let none = Vec::new;
    let rest = || vec![Spec::Rest];
    match sub {
        "look" => vec![Spec::Word("on", rest()), Spec::Word("off", none())],
        "glow" | "baby" | "sneak" | "invisible" | "shadow" | "seethrough" => words(&["on", "off"], none),
        "billboard" => words(BILLBOARDS, none),
        "align" => words(ALIGNMENTS, none),
        "background" | "permission" | "text" => vec![Spec::Word("none", none()), Spec::Rest],
        "equip" => words(SLOTS, || vec![Spec::Word("hand", Vec::new()), Spec::Word("none", Vec::new()), Spec::Rest]),
        "action" => vec![
            Spec::Word("add", words(&["any", "left", "right"], || DO_NAMES.iter().map(|(name, _)| Spec::Word(name, vec![Spec::Rest])).collect())),
            Spec::Word("list", none()),
            Spec::Word("clear", none()),
            Spec::Word("remove", rest()),
        ],
        "type" => commands::body_types().into_iter().map(|t| Spec::Word(t, Vec::new())).collect(),
        "particle" => words(names::PARTICLES, none),
        "attach" => vec![Spec::Name("npc", vec![Spec::Rest])],
        "remove" | "info" | "tp" | "movehere" | "detach" => none(),
        _ => rest(),
    }
}

/// `/npc` or `/display` as a tree of words, so the game can complete them with Tab. Pumpkin
/// 0.2.2 crashes when a plugin answers a completion request itself, and does not hand a plugin
/// the game's own item and block arguments, so only fixed words are offered: ids, items and
/// blocks are typed by hand.
fn command_tree(group: Group, names: &[String]) -> Command {
    let mut command = Command::new(names, if group == Group::Npc { "Create and change NPCs" } else { "Create and change displays" });
    let mut top: Vec<Spec> = commands::PLAIN_SUBS.iter().map(|sub| Spec::Word(sub, Vec::new())).collect();
    let kinds = match group {
        Group::Npc => {
            let mut kinds = vec![Spec::Word("player", vec![Spec::Rest]), Spec::Word("block", vec![Spec::Rest]), Spec::Word("item", vec![Spec::Rest])];
            kinds.extend(commands::body_types().into_iter().map(|t| Spec::Word(t, Vec::new())));
            kinds
        }
        Group::Display => vec![Spec::Word("text", vec![Spec::Rest]), Spec::Word("item", vec![Spec::Rest]), Spec::Word("block", vec![Spec::Rest]), Spec::Word("particle", words(names::PARTICLES, Vec::new))],
    };
    top.push(Spec::Word("create", vec![Spec::Name("id", kinds)]));
    top.extend(commands::id_subs(group).into_iter().map(|sub| Spec::Word(sub, vec![Spec::Name("id", after_id(sub))])));
    for spec in &top {
        command = command.then(node(group, spec, &[]));
    }
    command.execute(ThingCommand { group, path: vec![Part::Word("help")] })
}

/// What config.toml says. The file is written with its defaults the first time, and settings a
/// newer version adds are put in when they are missing.
struct Settings {
    console_colours: bool,
    /// The web editor's port; 0 for no web editor.
    web_port: u16,
    web_address: String,
}

const DEFAULT_SETTINGS: &[(&str, &str, &str)] = &[
    ("console_colors", "true", "Colours in the console, like Pumpkin's own output. Set to false for plain text."),
    ("web_port", "8787", "The port of the web editor (/npc web). 0 switches the editor off."),
    ("web_address", "\"\"", "The address to put in the editor's link, such as \"play.example.com\". Leave empty to use\n# the address the player connected with."),
];

fn settings(folder: &str) -> Settings {
    let path = format!("{folder}/config.toml");
    let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| "# cascade settings\n".to_string());
    let read = |text: &str, key: &str| {
        text.lines().find_map(|line| {
            let line = line.split('#').next().unwrap_or("");
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then(|| value.trim().trim_matches('"').to_string())
        })
    };
    let before = text.clone();
    for (key, value, about) in DEFAULT_SETTINGS {
        if read(&text, key).is_none() {
            text.push_str(&format!("\n# {about}\n{key} = {value}\n"));
        }
    }
    if text != before {
        let _ = std::fs::write(&path, &text);
    }
    Settings {
        console_colours: read(&text, "console_colors").as_deref() != Some("false"),
        web_port: read(&text, "web_port").and_then(|p| p.parse().ok()).unwrap_or(8787),
        web_address: read(&text, "web_address").unwrap_or_default(),
    }
}

struct Cascade;

impl Plugin for Cascade {
    fn new() -> Self {
        Self
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "cascade".into(),
            version: VERSION.into(),
            authors: vec!["Xathr".into()],
            description: "NPCs of any kind and displays of text, items, blocks and particles, all clickable.".into(),
            dependencies: vec![],
            permissions: vec![
                permissions::FS_READ_DATA.into(),  // things.json
                permissions::FS_WRITE_DATA.into(), // things.json
                permissions::NETWORK_TCP.into(),   // the web editor's port
            ],
        }
    }

    fn on_load(&self, context: Context) -> pumpkin_plugin_api::Result<()> {
        let data = context.get_data_folder();
        let _ = std::fs::create_dir_all(&data);
        let settings = settings(&data);
        color::set_enabled(settings.console_colours);
        WEB_ADDRESS.with(|a| *a.borrow_mut() = settings.web_address.clone());
        FOLDER.with(|f| *f.borrow_mut() = data);
        let count = match load() {
            Ok(n) => n,
            Err(e) => {
                warn(&e);
                0
            }
        };
        let (npcs, displays) = STORE.with(|s| {
            let store = s.borrow();
            (store.things.iter().filter(|t| t.group == Group::Npc).count(), store.things.iter().filter(|t| t.group == Group::Display).count())
        });
        let _ = count;
        for line in crate::banner::lines(&crate::banner::Info { version: VERSION, npcs, displays, file: "plugins/data/cascade/things.json" }) {
            info(&line);
        }

        let _ = context.register_permission(&Permission { node: "cascade:admin".into(), description: "Create and change NPCs and displays".into(), default: PermissionDefault::Op(PermissionLevel::Two), children: vec![] });
        context.register_command(command_tree(Group::Npc, &["npc".to_string()]), "admin");
        context.register_command(command_tree(Group::Display, &["display".to_string()]), "admin");
        context.register_event_handler::<PlayerInteractEntityEvent, _>(OnInteract, EventPriority::High, true)?;
        context.register_event_handler::<PlayerInteractUnknownEntityEvent, _>(OnInteractUnknown, EventPriority::High, true)?;
        context.register_event_handler::<EntityDamageByEntityEvent, _>(OnDamage, EventPriority::High, true)?;
        if settings.web_port != 0 {
            match crate::web::Web::start(settings.web_port) {
                Ok(web) => {
                    info(&format!("  {} {}", color::dim(&format!("{:<13}", "web editor")), format!("{} {}", color::value(&format!("port {}", settings.web_port)), color::dim("(/npc web gives the link)"))));
                    WEB.with(|w| *w.borrow_mut() = Some(web));
                }
                Err(e) => warn(&format!("the web editor is off: could not open port {} ({e}). It needs the network.tcp permission; web_port = 0 in config.toml silences this.", settings.web_port)),
            }
        }
        context.schedule_repeating_task(20, 1, every_tick);
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> pumpkin_plugin_api::Result<()> {
        unbuild_all();
        Ok(())
    }
}

register_plugin!(Cascade);
