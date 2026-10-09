# Game Design Document

Last updated: 2026-10-08

## Vision and pillars

An online, persistent block-world game in which players build settlements, haul finite resources over rails and bots, and cooperate or fight over a shared monument that ends each season. It borrows Clonk's settlement, logistics and throwing-and-explosives action, Factorio's goal-driven automation at small scale, and Diablo/Sacred's isometric look.

Pillars:

1. **Block world.** Everything is blocks. This is the engine's constraint and the game's identity.
2. **Isometric, real-time terrain.** Fixed-angle camera, dynamic light and shadow, detail in textures and sprites.
3. **Scarcity.** Resources are finite and consumed. Scarcity creates the social game.
4. **Small-scale logistics.** A few rail lines and mining bots, not factory sprawl.
5. **Asynchronous and stress-free.** Nobody has to be online. Being away costs at most a bounded, recoverable setback, and returning should feel like a reward.
6. **Progression through the MMO.** Power comes from trade, shared infrastructure and territory, not from a gear treadmill.

## Camera and rendering

The camera is isometric, and terrain is rendered in real time by the existing block engine. This was decided after weighing baked sprite terrain against real-time blocks.

- **Terrain:** real-time 3D blocks with the engine's AO and sun shadows. Per-block detail comes from high-resolution textures, normal maps, decals and optional parallax, all of which work at any camera angle.
- **Shared sun:** one sun state for the whole server, the same for every player, and it turns. Baked per-block lighting is therefore not an option, which confirms real-time terrain.
- **Why not baked terrain:** blocks are editable anywhere, and shadows from distant structures and a turning sun can't live in a per-block sprite.
- **Main character:** a real-time 3D skeletal mesh, not prerendered sprites. A mesh faces any angle, which mouse aiming needs, and animates smoothly at high frame rates (about 100 Hz) without a sprite sheet per facing and frame. It goes through the same lighting and shadow pass as the terrain, so it needs no proxy shadow.
- **Other entities and props:** bots, trains, buildings and props may be prerendered high-detail sprites with several facing directions (the Sacred approach), or meshes through the same pipeline. Undecided, see open questions.
- **Sprite shadows:** sprites are grounded by a soft contact blob that is pushed away from the sun and lengthens as it sinks (`wurfel-web/src/spriteshadow.rs`). Sprites already cast onto the terrain as camera-facing cut-out cards in the sun's shadow map (in the voxel method only sprites go through the map), which reads well except when the sun's direction lies in the card's plane and the card's shadow thins to a line.
- **Known gap:** a real volume for sprites in the sun shadow field is not done. A proxy box or capsule stamped into the map would fix the thin card shadow, but the shader lights a sprite at its foot with the same map, so the proxy shades the sprite it belongs to, and fixing that needs a second map or an id per caster. Other options are sprites with depth and normal maps, or meshes for entities.
- **Ground cover:** grass blades, pebbles and rocks are one system with one switch and one density. Pebbles and rocks are scattered over dirt, sand, stone and block borders by deterministic noise (`wurfel-sim/src/detail.rs`). Flowers, mushrooms, ferns, fallen leaves, twigs and clover are waiting for art.
- **Readability:** the world is only a few layers tall, so a height cutaway and fading of walls and roofs near the player is enough.

## World and terraforming

The world is a thin slab: any block can be edited, but total height is limited, so there is no deep digging. Everyone plays on one shared map.

- **Editing:** players hit blocks to destroy them and place blocks to build. Hit boxes select the visible block. Terraforming means walls, trenches, ramps, bridges, platforms and moving water, all readable from an isometric view.
- **Cost:** placing and removing blocks uses materials, so shaping terrain is an economic decision.
- **Resources are spread horizontally:** ore outcrops, cliffs and regions, not depth. Difficulty rises with distance and region rather than with depth.
- **Layer count:** not yet fixed. About 3 to 6 layers keeps the cutaway trivial. About 10 to 16 needs proper layer fading and careful sprite sorting.

## Core loop

A settlement turns finite resources into defence, expansion and a shared monument, and the loop restarts each season.

1. **Gather** finite resources from regions and deposits.
2. **Haul** them by rail and bot, and protect the lines.
3. **Refine** them into building materials and combat supplies (gunpowder, dynamite, arrows, turrets, drones).
4. **Spend** them on defence, expansion and contributions to the monument.
5. **Season ends** when a monument completes. Deposits respawn, new regions open, and progress is kept as a lasting record.

## Combat

Combat is basic and mostly an extension of logistics, as in Clonk.

- **Hitting is simple.** Melee is a plain hit. Aiming a hit in an isometric 3D world has many more dimensions than in Clonk's 2D side view (height, facing, blocks in the way), so rich melee is hard and is not a focus.
- **Throwing and explosives carry the action.** Players throw items and use dynamite, and craft ammo, turrets and drones. Fighting well depends on what you produced and brought.
- **Combat is pressure on the loop, not its reward.** Enemies guard deposits and threaten logistics lines. Fights don't drop gear upgrades.
- **Diablo/Sacred inspiration is visual** (isometric presentation), not a skill-and-cooldown combat system.

## Economy: scarcity and sinks

Resources are finite and consumed, as in Clonk, so territory and supply routes decide who prospers.

- **Finite deposits.** Ore and other deposits run out. Controlling them is the main reason players interact.
- **Continuous sinks.** Ammo, gunpowder, dynamite, turret and drone upkeep, ward fuel, repairs and terraforming drain materials for as long as threats exist. Sinks should outpace production over a season.
- **Replenishment.** A persistent world with finite resources needs a source. The current plan is the season reset when a monument completes. Regrowth of deposits, new regions and decay of abandoned structures returning materials are candidates for inside a season.
- **Offline production is capped.** Bots can fill storage to its capacity and then idle, so logging in is rewarded but a long absence is not punished.

## Logistics and automation

Automation is small-scale: a few rail lines and mining bots, in the spirit of Clonk, not factory sprawl.

- **Progression:** early on, players carry goods by hand. Rails, lifts and bots take over hauling as the settlement grows.
- **Mining bots** work deposits and fill capped storage, and they keep working while the owner is away.
- **Rails** connect deposits, refineries and outposts. Lines can be raided or sabotaged, so supplying a remote outpost is a real cost.
- **Distance matters.** Holding a faraway deposit means keeping its defences supplied, which makes expansion a trade-off.

## Defence: ward, local turrets, detection, traps

A settlement is protected by what it has stocked locally, and a sleeping base is only as strong as its fuel.

- **Awake and sleeping states.** While the owner is online the settlement is awake: the owner and their defences protect it directly. After logout it enters a sleeping state, sealed by a ward powered by a core that draws fuel from the local stockpile.
- **Ward under attack** burns fuel faster. When fuel runs out, the base opens. Offline attacks are resolved by calculation (attacker strength against defence strength and stock), not by live simulation.
- **Combat-logout guard.** The ward takes about 10 to 15 minutes to power up after logout, and can't engage while the base is under attack or recently damaged.
- **Local defences.** Turrets, walls and drones are destructible and run on local ammo, so each zone defends itself even when the owner is online elsewhere on the map.
- **Detection and traps.** Tripwires, pressure plates and sensors raise in-game alerts on the map, and traps slow raiders and cost them resources. Defences buy time rather than guarantee safety.
- **No alerts that demand a response.** Offline players are not pushed notifications about raids. They get a calm return report instead.
- **Anti-hoarding.** Upkeep drains stock slowly even without attacks, and defence-linked storage is capped.

## PvP: safe core, contested frontier

PvP is in v1, confined to the frontier so that being away never means losing everything.

- **Safe core.** A home settlement can't be wiped out by other players. Its vault is protected.
- **Contested frontier.** Outposts, deposits, border regions and monument sites are open to PvP. Players choose their exposure by how far they expand.
- **What a raid can do.** Destroy exposed structures (turrets, walls, bots, outposts) and take a capped share of the stock at the raided outpost.
- **Bounded and recoverable.** Losses are rebuildable. Each raided outpost gets a recovery cooldown, and new players get a protection period.
- **Offline outposts** hold by ward fuel, turrets and traps, with raids resolved by calculation, so no one has to log in to defend.

## Monument and seasons

A monument is the season's goal, in the manner of Factorio's rocket or Age of Empires' wonders, and its completion ends the season.

- **Building it** takes large contributions of specific resources, which gives the economy a visible purpose.
- **Season end.** When a monument completes, the world resets or advances: deposits respawn, new regions open, and progress is kept as a lasting record such as names, titles or cosmetics. This is the main answer to finite resources in a persistent world.
- **Contest.** Because everyone shares one map, the likely shape is one shared monument per season that factions build toward or contest. Not yet confirmed.
- **Season length** sets the pacing of every other system. Not yet chosen.

## Decisions so far

- Isometric camera, real-time block terrain.
- One shared map for all players.
- One shared sun state for all players, and the sun turns.
- The main character is a 3D skeletal mesh; other entities are undecided.
- Combat is basic hitting plus throwing and explosives, driven by logistics.
- Asynchronous, stress-free play; PvP confined to the frontier.

## Open questions

- [ ] How many layers is the height limit? This sets the cutaway and how tall buildings can be.
- [ ] Other entities (bots, trains, buildings, props) as prerendered sprites or as meshes? Sprites have a contact blob and a card shadow; a volume proxy that does not shade its own sprite is still open.
- [ ] Does `wurfel-web` support skinned, animated meshes, and what is the model and rigging pipeline for the character?
- [ ] One shared monument per season, or one per faction?
- [ ] How long is a season?
- [ ] Which replenishment mechanisms run inside a season (regrowth, new regions, decay)?
- [ ] Target player count on the single map, which sets the simulation budget.
- [ ] Exact protection rules for new players and recovery cooldowns.
- [ ] How are thrown items and explosives aimed and resolved in the isometric view?
