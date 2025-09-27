# Implementation Roadmap — Cosmic Crunchers

This document translates design.md into concrete implementation tasks, grouped into phases with acceptance criteria and suggested priorities. Treat this as a living roadmap; break tasks into smaller issues in your tracker (GitHub/GitLab) as you implement.

Guiding principles
- Start small and iterate: get a minimal authoritative server + client round-trip working before adding physics or complex AI.
- Server authoritative: simulation and game rules run on the server. Client renders + predicts.
- Keep systems testable and deterministic where possible (record/replay).
- Prioritize networking/lobby/simulation basics first — they form the backbone for all gameplay.

Phases overview
- Phase 0 — Project scaffolding & CI
- Phase 1 — Lobby, connections, and minimal netcode loop
- Phase 2 — Server simulation loop + ECS + physics integration
- Phase 3 — Client prototype (render, input, prediction)
- Phase 4 — Weapons, projectiles, enemies & bosses
- Phase 5 — Economy, shop, upgrades, drops
- Phase 6 — Persistence, auth, matchmaking
- Phase 7 — Observability, testing, scaling, polish
- Phase 8 — Release prep & post-launch ops

Phase 0 — Project scaffolding & CI
- [x] Create repository layout
  - /server — Rust crate
  - /client — TypeScript + Phaser 3 app
  - /docs — design & operational docs
  - /tools — scripts for testing, bots, record/replay
- [x] Add README with quick start for local dev
- [x] Create initial Rust server crate (cargo workspace if desired)
- [x] Create initial client scaffold (Vite + TypeScript or similar)
- [x] Add GitHub Actions CI:
  - Rust unit tests
  - Node linter/build for the client
  - Basic integration job later for server+client smoke test
- [x] Developer environment bootstrap (dev_setup.sh)
  - Add `dev_setup.sh` to validate and optionally install required tooling:
    - git, curl, wget, jq, make, cmake
    - rustup/cargo (Rust stable), rustfmt, clippy
    - node (prefer via nvm) and npm (Node >= 22)
  - Support common platforms (Linux package managers: apt/dnf/pacman) and macOS (brew). Prompt before installing or modifying shell profiles.
  - Script prints a summary, provides PATH/shell hints, and exits non-zero if critical dependencies remain missing.
  - Acceptance: Running `./dev_setup.sh` completes with required tools verified or installed (or exits non-zero with instructions). Add usage notes to README.
- Acceptance: `cargo build` in /server and `npm run dev` in /client succeed locally.

Phase 1 — Lobby, WebSocket connections, minimal netcode
- [ ] Implement axum HTTP + WebSocket endpoint(s)
  - Connection handshake: protocol version negotiation, player metadata submission
  - Binary frames only
- [ ] Implement lobby manager
  - Create/join by 8-char room code
  - Room lifecycle (create, expire, shutdown on empty)
  - Max players enforcement (10)
  - Room listing and optional public match
- [ ] Connection lifecycle & heartbeats
  - Ping/pong or heartbeat messages
  - Rejoin flow with grace window (120s)
- [ ] Simple per-lobby task loop
  - Spawn async task per room which can accept inputs and send binary snapshots
- [ ] Lightweight snapshot & input wire format
  - Define versioned rkyv schemas for input and snapshot (initial draft)
- [ ] Create a start script that launches the game server and prints the URL to visit to play the game.
- Acceptance:
  - Clients can open WebSocket and join a room.
  - Server receives timestamped inputs and can send binary snapshots that client can deserialize.

Phase 2 — Server simulation loop, ECS, Rapier2D
- [ ] Integrate hecs for entity management
  - Define initial components per design (Transform, Velocity, Health, Player, InputBuffer, etc.)
- [ ] Implement server sim loop
  - Target sim tick: 30 Hz
  - Input apply → movement integration → physics → collision resolution → game logic
- [ ] Integrate Rapier2D on server
  - World setup, collision layers, CCD for fast projectiles
  - Map hecs entities to rapier bodies/ colliders
- [ ] Deterministic update tooling
  - Input recorder and deterministic replayer for regression tests
- [ ] Snapshot builder & delta compressor
  - Build periodic snapshots (12 Hz default) using rkyv serialization
  - Implement seq numbers and ack semantics
- Acceptance:
  - Server runs a stable sim loop without crashing; tick times are within acceptable bounds on dev machine.
  - Deterministic replay works for short recorded sessions.

Phase 3 — Client prototype: rendering & input prediction
- [ ] Bootstrap Phaser 3 scene for arena
  - Render ships, projectiles, basic sprites/placeholders
- [ ] Input capture and local prediction
  - Sample input at 30 TPS, send to server, buffer locally with seq ids
  - Predict local ship motion using simplified kinematics
- [ ] Interpolation of remote entities
  - Implement fixed-lag interpolation (120 ms buffer)
- [ ] Snapshot handling & reconciliation
  - Apply server authoritative positions; smoothly correct local drift
- [ ] Debug overlay
  - Show netgraph (RTT, snapshot age), reconciliation corrections, entity count
- Acceptance:
  - Two clients connected to same room see their ships and can move; local client experiences immediate responsiveness via prediction while authoritative snapshots resolve corrections.

Phase 4 — Weapons, projectiles, enemies & bosses
- [ ] Implement primary weapon archetypes (rapid projectile + beam + spread)
  - Server-side projectile spawn, damage, lifetime
  - Client-side visuals and sound placeholders
- [ ] Implement secondary weapons (start with 2 types)
  - Homing missile (guidance) and area nuke (AoE)
  - Ammo limits and optional cooldown
- [ ] Enemy AI base system
  - Implement chaser and shooter archetypes
  - Spawner system for waves
- [ ] Boss framework
  - Multi-phase skeleton, weak point system, enrage timer
- [ ] Status effects & hit indicators
- Acceptance:
  - Enemies spawn in waves, can damage players, and drop loot upon death. Boss fight triggers on schedule and behaves according to phase logic.

Phase 5 — Economy, shop & upgrades
- [ ] Implement point/credit system and drop tables (ammo, fuel, shards)
- [ ] In-match shop UI (between waves)
  - Purchase ammo, fuel, one-time-use gadgets
- [ ] Persistent unlocks support
  - Cosmetic unlocks and ship unlocks in redb
- [ ] Pity mechanics & anti-hoarding
- Acceptance:
  - Players collect drops, spend points in between waves, and persistent unlocks are recorded in redb.

Phase 6 — Persistence, auth, matchmaking
- [ ] redb integration for profiles and progression
  - CRUD profile endpoints for meta (name, cosmetics, progression)
- [ ] Auth approach
  - Guest sessions initially; optional OAuth/email for accounts
- [ ] Matchmaking service (optional)
  - Public queue and auto-room creation
- [ ] Host migration / cross-server room handling plan
  - If rooms will be sharded later, document event/hand-off strategy (Redis/Kafka)
- Acceptance:
  - Profiles persist across restarts; user can rejoin a room and their profile loads.

Phase 7 — Observability, testing & anti-cheat
- [ ] Add tracing + metrics (Prometheus)
- [ ] Logging structured JSON, per-lobby metrics (tick latency, snapshot size)
- [ ] Deterministic unit and integration tests with recorded inputs
- [ ] Bot clients for soak testing
- [ ] Anti-cheat validations on server (fire rate, resource mutations, impossible movement)
- Acceptance:
  - Metrics are visible via local Prometheus scrape; bots can run a continuous match for N minutes.

Phase 8 — Scaling, deployments & polish
- [ ] Dockerize server & add a simple docker-compose for local cluster testing
- [ ] Add load testing harness (spawn many bot rooms/clients)
- [ ] Performance tuning and sharding plan (shard by room)
- [ ] Client polish: art, SFX, accessibility, controller support
- [ ] Prepare release pipeline & staging
- Acceptance:
  - Deployment manifests available, and a small cluster can host multiple lobbies without severe performance degradation.

Cross-cutting tasks
- [ ] Wire protocol versioning & migration testing
- [ ] Feature flags for toggling experimental mechanics
- [ ] Security review (WebSocket misuse, malformed packets, DoS mitigations)
- [ ] UX passes (HUD polish, onboarding tutorial)
- [ ] Accessibility checklist: remappable keys, colorblind palettes, UI scaling, controller support

Suggested immediate next tasks (first sprint — 2 weeks)
- [ ] Phase 0 tasks: repo scaffolding, basic CI
- [ ] Phase 1 tasks: WebSocket endpoint, lobby manager, basic per-lobby task loop
- [ ] Phase 2 skeleton: hecs integration, simple sim loop that applies inputs to transform components and sends snapshots
- Acceptance for sprint:
  - Two clients can connect to the server, join the same room, send inputs, and see snapshot updates for player positions.

Developer notes & recommended commands (local setup)
- Create server:
  - cargo new server --bin
  - cd server && cargo add axum tokio hecs rapier2d rkyv redb tracing serde bincode
- Create client:
  - npm create vite@latest client -- --template vanilla-ts
  - cd client && npm install phaser
- Run locally:
  - Start server via `cargo run` in `/server`
  - Start client dev server via `npm run dev` in `/client`

Issue labeling & milestones
- Milestone: alpha (lobby + sim + basic client)
- Milestone: beta (weapons, enemies, persistence)
- Milestone: release (polish, scaling, stability)
- Use labels: `backend`, `frontend`, `netcode`, `ecs`, `physics`, `ci`, `infra`, `qa`

Acceptance criteria & definition of done
- For each issue: include automated tests or a reproducible manual test plan.
- No feature should be considered done without:
  - Server-side validation tests (unit/integration)
  - Client-side basic smoke test
  - Performance baseline recorded (tick time and snapshots)

Appendix — mapping design.md items to implementation areas
- Netcode + wire format → Phase 1 & 2
- ECS + Rapier2D → Phase 2
- Prediction/reconciliation → Phase 3
- Weapon/enemy behaviours → Phase 4
- Economy + shop → Phase 5
- Profiles/persistence → Phase 6
- Observability → Phase 7
