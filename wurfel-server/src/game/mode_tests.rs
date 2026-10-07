//! The engine's `Game` with a game mode (Caveland) playing on it.

use super::*;
use wurfel_sim::grid::from_iso;

fn caveland_game() -> Game {
    mode::install();
    Game::new(WorldSpec { map: "c".into(), map_id: "c".into(), slot: 0, generator: "caveland".into(), seed: 1 }).unwrap()
}

fn run(game: &mut Game, ticks: u32) -> Vec<ServerMsg> {
    let mut out = Vec::new();
    for _ in 0..ticks {
        game.tick();
        out.extend(game.drain_outbox());
    }
    out
}

#[test]
fn the_caveland_generator_is_played_with_caveland_rules_and_the_island_with_the_engine() {
    let game = caveland_game();
    assert_eq!(game.game_mode(), "caveland");
    assert_eq!(game.info().gamemode, "caveland");
    let id = game.player_info(0).map(|p| p.id).unwrap_or(0);
    assert!(matches!(game.welcome(id), ServerMsg::Welcome { gamemode, .. } if gamemode == "caveland"));
    let mut island = Game::island(1);
    assert_eq!(island.game_mode(), "engine");
    island.add_player();
    assert!(run(&mut island, 10).is_empty(), "the plain engine has nothing extra to say");
    let mut forced = Game::island(1);
    forced.set_game_mode("caveland");
    assert_eq!(forced.game_mode(), "caveland", "any map can be played with Caveland rules");
    forced.set_game_mode("whatever");
    assert_eq!(forced.game_mode(), "engine", "unknown modes are the engine");
}

#[test]
fn caveland_things_are_not_players_and_arrive_as_things() {
    let mut game = caveland_game();
    let me = game.add_player();
    let msgs = run(&mut game, 4);
    let ServerMsg::Snapshot { players, .. } = game.snapshot() else { unreachable!() };
    assert_eq!(players.iter().map(|p| p.id).collect::<Vec<_>>(), vec![me], "items and the robot are not in the player list");
    let things: Vec<ThingState> = msgs
        .iter()
        .filter_map(|m| if let ServerMsg::Things { things, .. } = m { Some(things.clone()) } else { None })
        .next_back()
        .expect("things were sent");
    let kinds: Vec<&str> = things.iter().map(|t| t.kind.as_str()).collect();
    assert!(kinds.contains(&"Torch") && kinds.contains(&"robot"), "{kinds:?}");
}

#[test]
fn caveland_clients_cannot_edit_blocks_directly_but_can_act() {
    let mut game = caveland_game();
    let me = game.add_player();
    run(&mut game, 4);
    let p = game.entities.get(me).unwrap().position;
    let (x, y) = from_iso(p.x, p.y);
    let edit = ClientMsg::SetBlock { x: x + 1, y, z: p.z as i32 + 1, block: Block::new(id::STONE, 0).raw() };
    assert_eq!(game.handle(me, edit), None, "digging and building go through the rules");
    // Actions are accepted and answered later by the tick; nonsense is ignored.
    assert_eq!(game.handle(me, ClientMsg::Action { name: "attack".into(), arg: 0 }), None);
    assert_eq!(game.handle(me, ClientMsg::Action { name: "nonsense".into(), arg: -5 }), None);
    run(&mut game, 30);
}

#[test]
fn only_the_host_may_run_caveland_commands() {
    let mut game = caveland_game();
    let (host, guest) = (game.add_player(), game.add_player());
    for id in [guest, host] {
        game.handle(id, ClientMsg::Command { line: "give Torch".into(), path: String::new() });
    }
    // Each answer goes to the player who asked, and nobody else hears of it.
    let answers: Vec<(u32, bool)> = [guest, host].into_iter().flat_map(|id| replies_ok(&mut game, id).into_iter().map(move |ok| (id, ok))).collect();
    let broadcast = game.drain_outbox();
    assert!(!broadcast.iter().any(|m| matches!(m, ServerMsg::ConsoleReply { .. })));
    assert!(!broadcast.iter().any(|m| matches!(m, ServerMsg::Rules { kind, .. } if kind == "console")));
    assert_eq!(answers, vec![(guest, false), (host, true)]);
    // The plain engine has no Caveland commands: its own console answers.
    let mut engine = Game::island(1);
    let me = engine.add_player();
    assert_eq!(engine.handle(me, ClientMsg::Command { line: "give Torch".into(), path: String::new() }), None);
    let answer = replies(&mut engine, me).pop().unwrap();
    assert_eq!(answer["ok"], false);
    assert!(answer["text"].as_str().unwrap().contains("command not found"), "{answer}");
}

/// The console answers kept for `id`, as `{ok, text, lines}` for short assertions.
fn replies(game: &mut Game, id: u32) -> Vec<serde_json::Value> {
    game.take_replies(id)
        .into_iter()
        .map(|m| match m {
            ServerMsg::ConsoleReply { lines } => {
                let ok = !lines.iter().any(|l| l.level == wurfel_sim::console::Level::Error);
                let text = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
                serde_json::json!({"ok": ok, "text": text, "lines": lines})
            }
            other => panic!("not a console reply: {other:?}"),
        })
        .collect()
}

fn replies_ok(game: &mut Game, id: u32) -> Vec<bool> {
    replies(game, id).iter().map(|r| r["ok"] == true).collect()
}

fn command(game: &mut Game, id: u32, line: &str) -> serde_json::Value {
    game.handle(id, ClientMsg::Command { line: line.into(), path: String::new() });
    let mut answers = replies(game, id);
    assert_eq!(answers.len(), 1, "one answer for '{line}'");
    answers.pop().unwrap()
}

#[test]
fn console_answers_reach_only_the_player_who_asked() {
    let mut game = Game::island(1);
    let (host, guest) = (game.add_player(), game.add_player());
    game.handle(guest, ClientMsg::Command { line: "auth wrong".into(), path: String::new() });
    assert!(game.take_replies(host).is_empty(), "the host does not see the guest's login");
    assert!(game.drain_outbox().iter().all(|m| !matches!(m, ServerMsg::ConsoleReply { .. })), "not broadcast");
    assert_eq!(replies_ok(&mut game, guest), vec![false]);
    // A player who leaves takes their unsent answers along.
    game.handle(guest, ClientMsg::Command { line: "printmap".into(), path: String::new() });
    game.remove_player(guest);
    assert!(game.take_replies(guest).is_empty());
}

#[test]
fn auth_with_the_admin_token_lets_a_guest_change_the_world() {
    let (token, _) = init_admin_token();
    let mut game = Game::island(1);
    let (_host, guest) = (game.add_player(), game.add_player());
    assert_eq!(command(&mut game, guest, "teleport 3 4")["ok"], false);
    let wrong = command(&mut game, guest, "auth nope");
    assert_eq!(wrong["ok"], false);
    assert!(!wrong["text"].as_str().unwrap().contains(token), "the token is never echoed");
    let right = command(&mut game, guest, &format!("auth {token}"));
    assert_eq!(right["ok"], true, "{right}");
    assert_eq!(command(&mut game, guest, "teleport 3 4")["ok"], true);
    // Leaving ends it: the id may come back as somebody else.
    game.remove_player(guest);
    assert!(!game.is_admin(guest));
}

#[test]
fn auth_stops_answering_after_too_many_wrong_tokens() {
    let (token, _) = init_admin_token();
    let mut game = Game::island(1);
    let (_host, guest) = (game.add_player(), game.add_player());
    for _ in 0..5 {
        command(&mut game, guest, "auth wrong");
    }
    assert_eq!(command(&mut game, guest, &format!("auth {token}"))["ok"], false, "locked out");
    assert!(!game.is_admin(guest));
}

#[test]
fn engine_commands_run_on_the_server_for_the_host() {
    let mut game = Game::island(1);
    let (host, guest) = (game.add_player(), game.add_player());
    // Changing the world is for the host.
    let refused = command(&mut game, guest, "teleport 3 4");
    assert_eq!(refused["ok"], false);
    assert!(refused["text"].as_str().unwrap().contains("permission denied"), "{refused}");

    let moved = command(&mut game, host, "teleport 3 4");
    assert_eq!(moved["ok"], true, "{moved}");
    let p = game.entities.get(host).unwrap().position;
    assert_eq!(from_iso(p.x, p.y), (3, 4));

    let map = command(&mut game, guest, "printmap 0 0 0 4 2");
    assert_eq!(map["ok"], true, "reading is for everybody: {map}");
    assert_eq!(map["lines"].as_array().unwrap().len(), 3, "a legend and two rows");

    game.handle(host, ClientMsg::Editor { on: true });
    let p = game.entities.get(host).unwrap().position;
    let (x, y) = from_iso(p.x, p.y);
    game.handle(host, ClientMsg::SpawnThing { kind: EDITOR_THING_KINDS[0].into(), pos: [p.x, p.y, p.z + 1.0] });
    assert_eq!(game.things.len(), 1);
    let killed = command(&mut game, host, "killall");
    assert_eq!(killed["text"], "disposed 1 entities");
    assert!(game.things.is_empty());

    let (cx, cy) = chunk_of(x, y);
    assert_eq!(command(&mut game, host, &format!("fillwithair {} {}", cx + 3, cy))["ok"], true);
    assert!(game.drain_outbox().iter().any(|m| matches!(m, ServerMsg::BlocksSet { .. })), "the clients hear of it");
    let top = (cx + 3) * wurfel_sim::CHUNK_SIZE_X;
    assert!((0..CHUNK_SIZE_Z).all(|z| game.world.get(top, cy * wurfel_sim::CHUNK_SIZE_Y, z).is_air()));

    let unknown = command(&mut game, host, "loadmap other");
    assert_eq!(unknown["ok"], false);
    // Client commands are not run here.
    assert_eq!(command(&mut game, host, "fullscreen")["ok"], false);
}

#[test]
fn moves_are_announced_as_action_events_with_the_rules_outcome() {
    let mut game = caveland_game();
    let me = game.add_player();
    run(&mut game, 120);
    let actions = |msgs: Vec<ServerMsg>| -> Vec<(String, bool)> {
        msgs.iter()
            .filter_map(|m| match m {
                ServerMsg::Rules { kind, data } if kind == "events" => Some(data.as_array().unwrap().clone()),
                _ => None,
            })
            .flatten()
            .filter(|e| e["t"] == "action" && e["player"] == me)
            .map(|e| (e["name"].as_str().unwrap().to_string(), e["ok"].as_bool().unwrap()))
            .collect()
    };
    game.handle(me, ClientMsg::Action { name: "attack".into(), arg: 0 });
    game.handle(me, ClientMsg::Action { name: "attack".into(), arg: 0 });
    game.handle(me, ClientMsg::Action { name: "throw".into(), arg: 0 });
    game.handle(me, ClientMsg::Action { name: "nonsense".into(), arg: 0 });
    let seen = actions(run(&mut game, 2));
    assert_eq!(
        seen,
        [("attack".to_string(), true), ("attack".to_string(), true), ("throw".to_string(), false)],
        "a throw without a prepared pose failed"
    );
}

#[test]
fn engine_games_ignore_actions() {
    let mut game = Game::island(1);
    let me = game.add_player();
    assert_eq!(game.handle(me, ClientMsg::Action { name: "attack".into(), arg: 0 }), None);
    assert!(game.drain_outbox().is_empty());
}

// ---------------------------------------------------------------- animated blocks, benchmark

fn ticks(game: &mut Game, n: u32) {
    for _ in 0..n {
        game.tick();
    }
}

fn above_a_player(game: &Game, id: u32) -> (i32, i32, i32) {
    let p = game.entities.get(id).unwrap().position;
    let (x, y) = from_iso(p.x, p.y);
    (x, y, 20)
}

#[test]
fn animated_blocks_are_stepped_by_the_tick_and_reach_the_clients_batched_once_per_cell() {
    let mut game = Game::island(5);
    let me = game.add_player();
    let cell = above_a_player(&game, me);
    game.world.set(cell.0, cell.1, cell.2, Block::new(id::WATER, 0));
    game.animate_block(cell, BlockAnimation::new(vec![0.1; 3], true, true)).unwrap();
    assert!(game.drain_outbox().is_empty(), "nothing moved yet");
    ticks(&mut game, 7); // 7 / 60 s: past the first frame
    let out = game.drain_outbox();
    assert_eq!(out.len(), 1, "one batch: {out:?}");
    match &out[0] {
        ServerMsg::BlocksSet { edits } => assert_eq!(edits, &vec![Edit { x: cell.0, y: cell.1, z: cell.2, block: Block::new(id::WATER, 1).raw() }]),
        other => panic!("{other:?}"),
    }
    assert!(game.drain_outbox().is_empty(), "drained");
    // Several changes before a drain collapse into the newest value of the cell.
    ticks(&mut game, 6);
    ticks(&mut game, 6);
    let out = game.drain_outbox();
    assert_eq!(out.len(), 1);
    match &out[0] {
        ServerMsg::BlocksSet { edits } => {
            assert_eq!(edits.len(), 1);
            assert_eq!(Block::from_raw(edits[0].block), game.world.get(cell.0, cell.1, cell.2));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_big_animation_is_split_into_bounded_messages_and_the_number_of_cells_is_capped() {
    let mut game = Game::island(5);
    game.add_player();
    for i in 0..MAX_SERVER_ANIMATED as i32 {
        game.world.set(i % 40, 30 + i / 40, 20, Block::new(id::WATER, 0));
        game.animate_block((i % 40, 30 + i / 40, 20), BlockAnimation::new(vec![0.05; 2], true, true)).unwrap();
    }
    assert!(game.animate_block((0, 0, 21), BlockAnimation::sea()).is_err(), "capped");
    assert_eq!(game.animated_count(), MAX_SERVER_ANIMATED);
    ticks(&mut game, 4);
    let out = game.drain_outbox();
    assert!(out.len() >= 2, "{} messages", out.len());
    for msg in &out {
        match msg {
            ServerMsg::BlocksSet { edits } => assert!(edits.len() <= MAX_ANIMATION_EDITS),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn the_sea_is_left_to_the_clients_so_water_chunks_stay_unmodified_and_quiet() {
    let mut game = Game::island(5);
    game.add_player();
    ticks(&mut game, 120);
    assert_eq!(game.animated_count(), 0);
    assert!(game.drain_outbox().is_empty(), "no animation traffic from the server");
}

fn benchmark_command(game: &mut Game, id: u32) {
    game.handle(id, ClientMsg::Command { line: "benchmark".into(), path: String::new() });
}

#[test]
fn the_benchmark_is_for_the_host_only_and_not_in_a_game_mode() {
    let mut game = Game::island(5);
    let host = game.add_player();
    let guest = game.add_player();
    assert!(game.spawn_benchmark_ball(guest).is_err());
    benchmark_command(&mut game, guest);
    assert_eq!(game.balls.len(), 0);
    benchmark_command(&mut game, host);
    assert_eq!(game.balls.len(), 1);
    assert_eq!(game.entity_count(), 3);

    let mut caveland = caveland_game();
    let me = caveland.add_player();
    assert!(caveland.spawn_benchmark_ball(me).is_err());
}

#[test]
fn balls_are_things_not_players_and_fall_and_bounce() {
    let mut game = Game::island(5);
    let host = game.add_player();
    game.spawn_benchmark_ball(host).unwrap();
    let ball = game.balls[0].0;
    let z0 = game.entities.get(ball).unwrap().position.z;
    ticks(&mut game, 30);
    assert!(game.entities.get(ball).unwrap().position.z < z0, "falling");
    let players = match game.snapshot() {
        ServerMsg::Snapshot { players, .. } => players,
        _ => unreachable!(),
    };
    assert_eq!(players.len(), 1, "the ball is not a player");
    let things = game.drain_outbox().into_iter().find_map(|m| match m {
        ServerMsg::Things { things, .. } => Some(things),
        _ => None,
    });
    let things = things.expect("balls are sent as things");
    assert_eq!((things.len(), things[0].kind.as_str(), things[0].id), (1, "Benchmark Ball", ball));
}

#[test]
fn the_benchmark_cannot_overload_the_server() {
    let mut game = Game::island(5);
    let host = game.add_player();
    game.spawn_benchmark_ball(host).unwrap();
    // The spawner shortens its interval over time; give it ample ticks and keep the clock "fast".
    for _ in 0..(BALL_LIFETIME_TICKS as u32 / 2) {
        game.step();
        game.last_tick_secs = 0.0;
        assert!(game.balls.len() <= MAX_BENCHMARK_BALLS);
        assert!(game.entity_count() <= MAX_ENTITIES_FOR_BENCHMARK);
    }
    assert!(game.balls.len() > 5, "it does add balls: {}", game.balls.len());
    for _ in 0..MAX_BENCHMARK_BALLS * 2 {
        let _ = game.spawn_benchmark_ball(host);
    }
    assert!(game.balls.len() <= MAX_BENCHMARK_BALLS);
    assert!(game.spawn_benchmark_ball(host).is_err() || game.balls.len() < MAX_BENCHMARK_BALLS);
    // Balls expire, and the host leaving stops the benchmark.
    game.tick += BALL_LIFETIME_TICKS + 1;
    game.update_benchmark();
    assert!(game.balls.len() <= 1, "old balls are gone: {}", game.balls.len());
    game.remove_player(host);
    assert!(game.benchmark.is_none());
}
