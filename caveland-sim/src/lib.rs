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
//!
//! # Not ported yet
//!
//! `MineCart` rails and passengers (only the body exists), `LiftBasket`/`LiftLogic`, construction
//! sites and kits, power station/cable/torch/turret/robot factory, `Portal`/`ExitPortal`,
//! `Spaceship`, `SpiderRobot`, `Vanya`, `Shopkeeper`, `Bird`, `Quadrocopter`, flags, grass bending.
//! Everything that is rendering, UI, sound playback, animation or camera (`CLGameView`, `CLCamera`,
//! menus, `ActionBox`, HUD) belongs to the client and is not simulation. The Caveland map
//! generator still lives in `wurfel_sim::caveland` because it is registered with the engine's
//! generator list.

pub mod ai;
pub mod barrier;
pub mod blocks;
pub mod collectible;
pub mod container;
pub mod crafting;
pub mod game;
pub mod logic;
pub mod player;
pub mod team;
pub mod tuning;

pub use blocks::{CavelandBlocks, ClBlock};
pub use collectible::{CollectibleType, Item};
pub use container::Inventory;
pub use game::{Caveland, EntityKind, GameEvent};
pub use player::{Action, Controls, PlayerState};
pub use team::Team;
pub use tuning::Tuning;
