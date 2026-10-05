//! Caveland's game rules on top of the Wurfel Engine simulation.
//!
//! Like the Java game, which is a separate project depending on the engine, this crate depends on
//! [`wurfel_sim`] and the engine does not know about it. The engine offers extension points; this
//! crate plugs into them:
//!
//! * [`blocks::CavelandBlocks`] implements the engine's `BlockConfig`, so the block ids of
//!   Caveland (oven, torch, ores...) mean the right thing to the engine's physics. Install it with
//!   [`Caveland::install`].
//! * Entities stay the engine's `Entity`. What makes one a player, a collectible or a robot is kept
//!   by [`Caveland`] in a table next to the engine's `Entities`, and [`Caveland::tick`] runs the
//!   rules around `Entities::update`.
//! * Behaviour that fits the engine's `Component` trait is one: [`barrier::ColumnBarrier`]
//!   (`CLMovableEntity`'s full-height wall) and [`ai::IdleAi`].
//!
//! # Ported from the Java game
//!
//! | Java | here |
//! |---|---|
//! | `CavelandBlocks`, `CLBlocks` | [`blocks`] |
//! | `CLMovableEntity.collidesWithWorld` | [`barrier`] |
//! | `Ejira` (health, inventory, jetpack, attacks, digging, interaction) | [`player`], [`game`] |
//! | `CollectibleType`, `Collectible`, `TFlint`, `TorchCollectible` | [`collectible`], [`game`] |
//! | `CollectibleContainer`, `Inventory` | [`container`] |
//! | `Recipe`, `CraftingRecipesList`, `CraftingDialogueBox` rules | [`crafting`] |
//! | `OvenLogic` | [`logic`] |
//! | `HasTeam`, `Robot`, `IdleAI`, `Money` | [`team`], [`ai`], [`game`] |
//! | the Caveland CVars | [`tuning`] |
//! | `Vanya`, `Shopkeeper`, `Bird`, the `ActionBox` dialogs as data | [`npc`], [`dialog`], [`extras`] |
//! | `ConstructionSite`, `ConstructionKit`, `InstantConstructionKit`, `DropSpaceFlag(ConstructionKit)` | [`construction`], [`extras`] |
//! | `AbstractPowerBlock`, `CableBlock`, `PowerStationLogic`, `PowerTorch`, `Turret` | [`power`], [`extras`] |
//! | `SpiderRobot`, `Quadrocopter`, `RobotFactory`, `RobotFactoryLinker` | [`enemy`], [`extras`] |
//! | `Flag`, `Flagpole`, the tutorial and end fight of `CLGameController` | [`construction`], [`extras`] |
//! | `GiveCommand`, `TeleportPlayerCommand`, `PortalTargetCommand` | [`commands`] |
//! | `MineCart`, the rails of `CLBlocks.RAILS`/`RAILSBOOSTER`, `BoosterLogic`'s power check | [`minecart`], [`rails`], [`power`] |
//! | `LiftBasket`, `LiftLogic`, `LiftLogicGround` | [`lift`] |
//! | `Portal`, `ExitPortal`, `CaveEntryBlockLogic` | [`portal`] |
//! | `Spaceship` | [`spaceship`] |
//! | the glue of the four above: updates, boarding, teleporting, logic blocks | [`transport`] |
//!
//! # Not ported yet
//!
//! Grass bending, saving and loading the state of carts, baskets and portals (they are made again
//! from the map when it loads), and the light a cart carries (the client lights its lamp while
//! [`minecart::MineCart::on_rails`]). The intro cutscene is the spaceship's; the story only waits for it
//! ([`Caveland::set_tutorial_step`]).
//! Everything that is rendering, UI, sound playback, animation or camera (`CLGameView`, `CLCamera`,
//! menus, `ActionBox`, HUD) belongs to the client and is not simulation. The Caveland map
//! generator still lives in `wurfel_sim::caveland` because it is registered with the engine's
//! generator list.

pub mod ai;
pub mod barrier;
pub mod cells;
pub mod blocks;
pub mod collectible;
pub mod commands;
pub mod construction;
pub mod container;
pub mod crafting;
pub mod dialog;
pub mod enemy;
pub mod extras;
pub mod game;
pub mod lift;
pub mod logic;
pub mod minecart;
pub mod npc;
pub mod portal;
pub mod player;
pub mod power;
pub mod rails;
pub mod spaceship;
pub mod team;
pub mod transport;
pub mod tuning;

pub use blocks::{CavelandBlocks, ClBlock};
pub use collectible::{CollectibleType, Item};
pub use container::Inventory;
pub use dialog::{Dialog, DialogMode, DialogOption};
pub use extras::ExtraEvent;
pub use game::{Caveland, EntityKind, GameEvent, PlayerView, RecipeView};
pub use player::{Action, Controls, PlayerState};
pub use team::Team;
pub use transport::{Interaction, Transport, TransportEvent};
pub use tuning::Tuning;
