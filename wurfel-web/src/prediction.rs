//! Client-side prediction bookkeeping: what the local player pressed and for how many physics
//! steps, so the state the server reports can be replayed up to "now".
//!
//! The client runs the same fixed 60 Hz physics as the server, so the local player never waits for
//! a reply. The server only sends corrections. A snapshot says "after `ticks` physics ticks under
//! input number `seq` you were here". The client knows how many of its own steps each input lasted,
//! replays the steps the server has not simulated yet from that state, and compares the result with
//! where it actually is. The difference is the real prediction error (not the latency).

use std::collections::VecDeque;

use glam::Vec3;
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::player::{apply_input, PlayerInput, TICK_DT};
use wurfel_sim::World;

/// Above this distance (blocks) the server saw something else (blocked, pushed, teleported): jump
/// to its state instead of blending.
pub const SNAP_DISTANCE: f32 = 1.5;
/// Below this distance (blocks) the correction is not worth applying visibly.
pub const IGNORE_DISTANCE: f32 = 0.002;
/// How fast the visual error fades, per second (an exponential: the half-life is about 70 ms).
pub const BLEND_RATE: f32 = 10.0;

/// One input and the client step at which it started.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub seq: u32,
    pub start_step: u64,
    pub input: PlayerInput,
}

/// The inputs of the local player since the server's last acknowledgement.
#[derive(Debug, Clone)]
pub struct InputHistory {
    segments: VecDeque<Segment>,
    /// Physics steps run so far.
    step: u64,
}

impl Default for InputHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl InputHistory {
    /// Nothing pressed, step 0, input number 0 (the server's initial state too).
    pub fn new() -> Self {
        let mut segments = VecDeque::new();
        segments.push_back(Segment { seq: 0, start_step: 0, input: PlayerInput::default() });
        InputHistory { segments, step: 0 }
    }

    /// The input in effect.
    pub fn current(&self) -> PlayerInput {
        self.segments.back().expect("never empty").input
    }

    pub fn seq(&self) -> u32 {
        self.segments.back().expect("never empty").seq
    }

    /// Physics steps run so far.
    #[cfg(test)]
    pub fn steps(&self) -> u64 {
        self.step
    }

    /// Call right before running a physics step with the keys that are pressed now. If they differ
    /// from the input in effect, a new input starts at this step and is returned for sending.
    /// Changes are only committed here, so every input lasts at least one step on the client, like
    /// on the server.
    pub fn begin_step(&mut self, wanted: PlayerInput) -> Option<(u32, PlayerInput)> {
        if wanted == self.current() {
            return None;
        }
        let seq = self.seq() + 1;
        self.segments.push_back(Segment { seq, start_step: self.step, input: wanted });
        Some((seq, wanted))
    }

    /// Call after the physics step ran.
    pub fn end_step(&mut self) {
        self.step += 1;
    }

    /// The steps to replay on top of a server state that is `ack_ticks` ticks into input `ack_seq`:
    /// pairs of (input, number of steps), in order. Everything before the acknowledged input is
    /// forgotten. `None` if the acknowledged input is unknown (older than what was kept, or from
    /// the future), in which case nothing can be said about the error.
    pub fn replay_plan(&mut self, ack_seq: u32, ack_ticks: u32) -> Option<Vec<(PlayerInput, u32)>> {
        let first = self.segments.iter().position(|s| s.seq == ack_seq)?;
        self.segments.drain(..first);
        let mut plan = Vec::new();
        for i in 0..self.segments.len() {
            let segment = self.segments[i];
            let end = self.segments.get(i + 1).map_or(self.step, |next| next.start_step);
            let mut steps = (end - segment.start_step) as u32;
            if i == 0 {
                // The server already ran `ack_ticks` of them. If it ran more than we did (the
                // client started later than the server's count), there is nothing left to replay.
                steps = steps.saturating_sub(ack_ticks);
            }
            if steps > 0 {
                plan.push((segment.input, steps));
            }
        }
        Some(plan)
    }
}

/// Put the entity into the state the server reported and run the steps of `plan` on top of it
/// (`InputHistory::replay_plan`): the same input handling and physics step the client runs every
/// frame and the server runs every tick. Returns where the entity ends up, or `None` if it is gone.
pub fn replay(
    entities: &mut Entities,
    id: EntityId,
    world: &World,
    server_pos: Vec3,
    server_vel: Vec3,
    plan: &[(PlayerInput, u32)],
) -> Option<Vec3> {
    let entity = entities.get_mut(id)?;
    entity.position = server_pos;
    entity.body.as_mut()?.movement = server_vel;
    for &(input, steps) in plan {
        for _ in 0..steps {
            apply_input(entities.get_mut(id)?, input, world);
            entities.update(world, TICK_DT);
        }
    }
    Some(entities.get(id)?.position)
}

/// What to do with the difference between the replayed server state and the predicted one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Correction {
    /// Too small to matter.
    Ignore,
    /// Correct the simulation now and fade the visible jump out.
    Blend,
    /// The prediction was wrong: take the server's state as it is.
    Snap,
}

pub fn classify(error: f32) -> Correction {
    if error > SNAP_DISTANCE {
        Correction::Snap
    } else if error > IGNORE_DISTANCE {
        Correction::Blend
    } else {
        Correction::Ignore
    }
}

/// The part of a correction that has not been shown yet. The simulation is corrected at once; the
/// rendered position is simulation + this offset, which fades to zero, so the player glides
/// instead of jumping.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VisualOffset(Vec3);

impl VisualOffset {
    /// The simulation moved from `before` to `after`: keep drawing where it was.
    pub fn absorb(&mut self, before: Vec3, after: Vec3) {
        self.0 += before - after;
    }

    pub fn clear(&mut self) {
        self.0 = Vec3::ZERO;
    }

    pub fn decay(&mut self, dt: f32) {
        self.0 *= (-BLEND_RATE * dt).exp();
        if self.0.length_squared() < 1e-8 {
            self.0 = Vec3::ZERO;
        }
    }

    pub fn value(&self) -> Vec3 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn right() -> PlayerInput {
        PlayerInput { right: true, ..Default::default() }
    }

    fn jump() -> PlayerInput {
        PlayerInput { jump: true, ..Default::default() }
    }

    fn run(history: &mut InputHistory, input: PlayerInput, steps: u32) -> Vec<(u32, PlayerInput)> {
        let mut sent = Vec::new();
        for _ in 0..steps {
            sent.extend(history.begin_step(input));
            history.end_step();
        }
        sent
    }

    #[test]
    fn a_change_starts_a_numbered_input_and_is_sent_once() {
        let mut h = InputHistory::new();
        assert_eq!(run(&mut h, PlayerInput::default(), 3), vec![], "no change, nothing to send");
        assert_eq!(run(&mut h, right(), 5), vec![(1, right())], "sent once, not every step");
        assert_eq!(run(&mut h, jump(), 1), vec![(2, jump())]);
        assert_eq!((h.seq(), h.steps(), h.current()), (2, 9, jump()));
    }

    #[test]
    fn replay_covers_the_steps_the_server_has_not_run() {
        let mut h = InputHistory::new();
        run(&mut h, PlayerInput::default(), 2);
        run(&mut h, right(), 10); // input 1: steps 2..12
        run(&mut h, PlayerInput::default(), 4); // input 2: steps 12..16
        // The server is 6 ticks into input 1: 4 of its steps and all of input 2 are left.
        let plan = h.replay_plan(1, 6).unwrap();
        assert_eq!(plan, vec![(right(), 4), (PlayerInput::default(), 4)]);
    }

    #[test]
    fn replay_of_the_latest_input_is_its_unsimulated_tail() {
        let mut h = InputHistory::new();
        run(&mut h, right(), 30);
        assert_eq!(h.replay_plan(1, 22).unwrap(), vec![(right(), 8)]);
        assert_eq!(h.replay_plan(1, 30).unwrap(), vec![], "fully acknowledged: nothing to replay");
    }

    #[test]
    fn acknowledged_inputs_are_forgotten() {
        let mut h = InputHistory::new();
        run(&mut h, right(), 5);
        run(&mut h, jump(), 5);
        run(&mut h, PlayerInput::default(), 5);
        h.replay_plan(2, 1).unwrap();
        assert_eq!(h.replay_plan(1, 0), None, "input 1 is older than what is kept");
        assert_eq!(h.replay_plan(2, 1).unwrap(), vec![(jump(), 4), (PlayerInput::default(), 5)]);
    }

    #[test]
    fn a_server_ahead_of_the_client_replays_nothing_for_that_input() {
        // The server counted ticks since joining; the client's first input is shorter than that.
        let mut h = InputHistory::new();
        run(&mut h, PlayerInput::default(), 4);
        assert_eq!(h.replay_plan(0, 9).unwrap(), vec![]);
    }

    #[test]
    fn an_unknown_or_future_ack_cannot_be_replayed() {
        let mut h = InputHistory::new();
        run(&mut h, right(), 5);
        assert_eq!(h.replay_plan(7, 0), None);
    }

    #[test]
    fn errors_are_sorted_into_ignore_blend_and_snap() {
        assert_eq!(classify(0.0), Correction::Ignore);
        assert_eq!(classify(IGNORE_DISTANCE), Correction::Ignore);
        assert_eq!(classify(0.2), Correction::Blend);
        assert_eq!(classify(SNAP_DISTANCE), Correction::Blend);
        assert_eq!(classify(SNAP_DISTANCE + 0.01), Correction::Snap);
    }

    #[test]
    fn a_blended_correction_keeps_the_picture_still_then_fades() {
        let mut offset = VisualOffset::default();
        let before = Vec3::new(5.0, 5.0, 1.0);
        let after = Vec3::new(5.3, 5.0, 1.0);
        offset.absorb(before, after);
        // Simulation + offset is where the player was drawn a moment ago.
        assert_eq!(after + offset.value(), before);
        let mut last = offset.value().length();
        for _ in 0..30 {
            offset.decay(1.0 / 60.0);
            let now = offset.value().length();
            assert!(now < last, "shrinks every frame");
            last = now;
        }
        assert!(last < 0.3 * 0.25, "mostly gone after half a second: {last}");
        for _ in 0..600 {
            offset.decay(1.0 / 60.0);
        }
        assert_eq!(offset.value(), Vec3::ZERO);
    }

    #[test]
    fn corrections_add_up_instead_of_replacing_each_other() {
        let mut offset = VisualOffset::default();
        offset.absorb(Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO);
        offset.absorb(Vec3::new(0.0, 2.0, 0.0), Vec3::ZERO);
        assert_eq!(offset.value(), Vec3::new(1.0, 2.0, 0.0));
        offset.clear();
        assert_eq!(offset.value(), Vec3::ZERO);
    }

    // ----- the whole loop: a client that predicts, a server that is authoritative, and latency

    use wurfel_sim::generator::create_generator;
    use wurfel_sim::grid::to_iso;
    use wurfel_sim::player::new_player;

    /// A server player reduced to what reconciliation depends on: the input stream rules.
    #[derive(Default)]
    struct Server {
        input: PlayerInput,
        seq: u32,
        ticks: u32,
    }

    struct Snapshot {
        arrives: u32,
        pos: Vec3,
        vel: Vec3,
        seq: u32,
        ticks: u32,
    }

    fn standing_world() -> (World, Vec3) {
        let mut world = World::new(create_generator("island", 1).unwrap());
        world.load_area((0, 0), 2);
        let (gx, gy) = to_iso(0, 0);
        let z = wurfel_sim::entity::physics::ground_height(&world, 0, 0);
        (world, Vec3::new(gx, gy, z))
    }

    /// Runs `ticks` ticks. `shove` teleports the client's body once, as if its prediction was wrong.
    /// Returns the error measured at every snapshot (distance between replayed server state and
    /// the client's actual position), and the final client and server positions.
    fn simulate(latency: u32, ticks: u32, shove: Option<(u32, Vec3)>) -> (Vec<f32>, Vec3, Vec3) {
        let (world, start) = standing_world();
        let script = |t: u32| match t {
            10..=49 => PlayerInput { right: true, ..Default::default() },
            60..=64 => PlayerInput { jump: true, ..Default::default() },
            90..=130 => PlayerInput { up: true, left: true, ..Default::default() },
            _ => PlayerInput::default(),
        };
        let (mut client, mut remote) = (Entities::new(), Entities::new());
        let cid = client.spawn(new_player(start));
        let sid = remote.spawn(new_player(start));
        let (mut history, mut server) = (InputHistory::new(), Server::default());
        let mut uplink: Vec<(u32, u32, PlayerInput)> = Vec::new();
        let mut downlink: Vec<Snapshot> = Vec::new();
        let mut errors = Vec::new();

        for t in 0..ticks {
            // client step
            if let Some((seq, input)) = history.begin_step(script(t)) {
                uplink.push((t + latency, seq, input));
            }
            apply_input(client.get_mut(cid).unwrap(), history.current(), &world);
            client.update(&world, TICK_DT);
            history.end_step();
            if let Some((at, by)) = shove {
                if at == t {
                    client.get_mut(cid).unwrap().position += by;
                }
            }
            // server tick
            for &(arrives, seq, input) in &uplink {
                if arrives == t && seq > server.seq {
                    server.input = input;
                    server.seq = seq;
                    server.ticks = 0;
                }
            }
            apply_input(remote.get_mut(sid).unwrap(), server.input, &world);
            remote.update(&world, TICK_DT);
            server.ticks += 1;
            if t % 2 == 0 {
                let e = remote.get(sid).unwrap();
                downlink.push(Snapshot {
                    arrives: t + latency,
                    pos: e.position,
                    vel: e.body.as_ref().unwrap().movement,
                    seq: server.seq,
                    ticks: server.ticks,
                });
            }
            // client receives
            for snap in downlink.iter().filter(|s| s.arrives == t) {
                let before = client.get(cid).unwrap().position;
                let plan = history.replay_plan(snap.seq, snap.ticks).expect("acknowledged input is known");
                let after = replay(&mut client, cid, &world, snap.pos, snap.vel, &plan).unwrap();
                errors.push(before.distance(after));
            }
        }
        (errors, client.get(cid).unwrap().position, remote.get(sid).unwrap().position)
    }

    #[test]
    fn with_latency_and_no_surprises_the_replay_matches_the_prediction_exactly() {
        for latency in [1, 3, 6, 12] {
            let (errors, _, _) = simulate(latency, 240, None);
            assert!(errors.len() > 80, "snapshots were evaluated: {}", errors.len());
            let worst = errors.iter().cloned().fold(0.0, f32::max);
            assert!(worst < 1e-4, "latency {latency}: worst error {worst}");
        }
    }

    #[test]
    fn the_client_is_ahead_of_the_server_by_the_latency_not_behind() {
        // While walking the client has moved further than the server has seen.
        let (_, client, server) = simulate(6, 30, None);
        let (_, start) = standing_world();
        assert!(client.distance(start) > server.distance(start) + 0.2, "client {client:?} server {server:?}");
    }

    #[test]
    fn a_wrong_prediction_shows_up_as_exactly_that_error_once() {
        let shove = Vec3::new(0.3, 0.0, 0.0);
        let (errors, _, _) = simulate(6, 240, Some((20, shove)));
        let big: Vec<f32> = errors.iter().cloned().filter(|&e| e > 1e-3).collect();
        assert!(!big.is_empty(), "the displacement was noticed");
        assert!((big[0] - 0.3).abs() < 1e-3, "first correction is the displacement: {}", big[0]);
        // The first replay corrected the simulation: later snapshots agree again.
        assert!(big.len() <= 4, "corrected once, then consistent: {big:?}");
        assert!(errors.last().unwrap() < &1e-3);
    }
}
