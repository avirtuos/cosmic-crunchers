# Cosmic Crunchers — Design Document

Status: Draft — updated with expanded gameplay, netcode, and architecture details.

Overview
--------
Cosmic Crunchers is a multiplayer, browser-playable 2D space shooter where friends join a fixed-size game world (room) and cooperate to destroy waves of enemies and asteroids while collecting fuel, ammo, and points to buy upgrades. Every N minutes players face a Boss encounter. The server is authoritative for all game state; clients render, accept input, predict locally, and reconcile with server snapshots.

Architecture & Libraries
-------------------------------------
Backend (authoritative server)
- Language: Rust 
- Key libraries:
  - axum — HTTP + WebSocket routing
  - tokio — async runtime
  - hecs — ECS (entity-component-system)
  - rapier2d — physics & collision
  - rkyv — fast binary serialization for snapshots
  - redb — embedded persistence for profiles and meta (initially)
- Rationale: Rust provides low-latency, deterministic performance for simulation and networking. Rapier2D and hecs work well together and rkyv enables efficient snapshot encoding. 

Frontend (client)
- TypeScript + Phaser 3 (rendering, input, UI)
- Native WebSocket (binary frames)
- Client-side systems: input capture, local prediction for player ship, interpolation for other entities, UI/HUD, debug overlay

Persistence & Meta
- redb for server-side profiles, progression, lobbies initially.

High-level Netcode Decisions
----------------------------
- Server is authoritative for simulation & collision resolution.
- Rates:
  - Server simulation tick: 30 Hz (suggested default)
  - Client input send: 30 Hz
  - Server snapshots/deltas to clients: 10–15 Hz (default 12 Hz)
  - Interpolation buffer on client: ~100–150 ms
- Input model:
  - Clients send timestamped input commands with sequence numbers; server acks command sequences.
  - Client prediction for local ship (kinematic model); server reconciliation when authoritative state arrives.
- Snapshot format:
  - Binary, rkyv-serialized structured snapshots; consider bit-packing for hot paths.
- Reliability:
  - Use WebSocket binary frames. Implement light-weight ack/seq and packet bundling. Apply rate limiting per connection.
- Time sync: maintain a server-time offset estimate using ping/RTO measurement. Server timestamps snapshots.

Gameplay (Core)
---------------
Core loop
- Wave-based progression with periodic bosses. Sensible default:
  - Wave length: 120 seconds
  - Boss every 5 waves (~ every 10 minutes)
  - Between-wave shop: 60 seconds to spend points and upgrade
- Level display: top bar shows current level (increments per wave)
- Win/loss:
  - End conditions configurable: survive X waves, reach a score, or endless survival.
  - Default: endless with persistent score; optional match objectives later.
  - The server maintains a global high score list that spans all games and even server restarts.

World & Camera
- World: bounded arena (default 5000 × 5000 world units)
- Camera: centers on local player with a soft dead-zone; optional group-zoom for co-op or a minimap/radar for off-screen threats
- Boundary behavior: hard walls with visual/physics feedback (optional wrap mode later)

Movement & Controls
- Movement model default: thrust + rotation with inertia (Asteroids-like)
- Abilities: boost/dash (consumes fuel or adds heat), capped top speed, friction tuned for arcade feel

Weapons & Combat
- Primary weapons:
  - Multiple archetypes (rapid-fire projectile, beam with heat, spread/shotgun)
  - Large ammo reserves or overheat mechanics
- Secondary weapons:
  - Very powerful, limited ammo (<=10)
  - Types: homing missiles, area nuke, EMP, railgun. Each has optional cooldown in addition to ammo.
- Status effects: slow, burn (DoT), stun/EMP, armor-shred
- Weapon UX: clear visual telegraphs for large/area effects; hit markers and audio feedback
- Hit detection: server resolves collisions using Rapier2D; client-side continuous checks for visuals

Economy, Upgrades & Drops
- Points: earned by destroying enemies and completing objectives; currency for in-match shop and persistent unlocks
- In-match shop (between waves):
  - Buy ammo, fuel, temporary weapon boosts, and one-time-use gadgets
- Persistent progression:
  - Cosmetic unlocks, unlocked ship types, permanent stat upgrades (optional)
- Drops:
  - Ammo/fuel guaranteed at predictable rates with “pity” mechanics (increasing chance when resources are low)
  - Default drop rates (tunable):
    - Ammo: 20%
    - Fuel: 15%
    - Secondary ammo shard: 5%
    - Credits/shards: 30%
  - Pity: +10% chance per 5s when player resource below 20%
- Anti-hoarding:
  - Per-match soft caps, diminishing returns to discourage stagnation

Enemies & Bosses
- Enemy archetypes:
  - Chaser (close-range), Shooter (ranged), Tank (high HP/slow), Splitter (spawns smaller enemies), Mine-layer (stationary hazards), Support (heals or buffs others)
- Bosses:
  - Multi-phase with weak points, telegraphed attacks, adds, and enrage timer at high durations
  - Guaranteed loot on defeat (ammo, fuel, and a rare shard)
- Difficulty scaling:
  - Per-level scaling knobs: spawn rate, enemy HP, armor, projectile speed, AI aggressiveness, and drop rates
  - Balance to avoid unkillable bosses (cap HP scaling or adjust damage exposure)

Co-op & Team Mechanics
- Max players per room: 10
- Team mechanics:
  - Shared objectives with combo multipliers for simultaneous kills
  - Revive mechanic: players can revive teammates by spending time near them (3s)
  - Damage sharing options for higher difficulties
- Friendly fire:
  - Off by default; optional toggle for hardcore modes

UI, HUD & Feedback
- Top bar: level, player name, health, shield, primary weapon level and ammo, secondary weapon and remaining ammo
- Boss UI: boss name, HP bar, phase indicators
- Network debug: ping, packet loss, snapshot age (toggleable)
- Offscreen indicators: markers for threats, loot, and teammates
- Accessibility: colorblind palettes, UI scale, rebindable keys

Match Flow & Lobbies
- Lobby model:
  - 8-digit room code (alphanumeric) for private games
  - Option for public matchmaking later
  - Host migration: server authoritative model avoids host dependency, but handle disconnect/rejoin gracefully
  - Late join: allow late joiners into an active match with a respawn penalty or restriction (configurable)
  - Rejoin window: 60–120s grace to rejoin same profile to prevent permanent dropout
- Match persistence:
  - Profiles: guest vs account based systems
  - What persists: cosmetics, unlocked weapons, player stats. In-match state does not persist.

ECS Schema (Initial Components)
- Transform (pos, rot)
- Velocity (linear, angular)
- InputBuffer (recent inputs + seq ids)
- Player (player_id, presence info)
- Ship (ship_type, class)
- Health, Shield
- Fuel, AmmoPrimary, AmmoSecondary
- PrimaryWeapon, SecondaryWeapon
- Projectile (owner, damage, lifetime)
- Collider (rapier handle / shape)
- Enemy (archetype, ai_state)
- Boss (phase, enrage_timer)
- LootDrop (type, value)
- Score
- Lifetime (ttl)
- SpawnPoint
- DamageEvent

Core Systems (Order & Responsibilities)
- Input ingestion & validation
- Movement integration / kinematic step
- Rapier2D physics step & collision resolution
- Projectile lifecycle & collision handling
- Damage application & health/shield updates
- Loot spawn and pickup handling
- AI behavior and enemy spawner systems
- Wave & boss spawner manager
- Snapshot builder / delta compressor
- Cleanup and GC (expired entities)
- Persistence writers (profiles, progression) — async & rate-limited

Rapier2D & Physics Notes
- Units: 1 world unit = 1 meter (tunable)
- Use CCD (continuous collision detection) for fast bullets or use raycasts for instant-hit weapons
- Separate collision layers: players, player-projectiles, enemies, enemy-projectiles, environment
- Keep physics determinism in mind for server-only simulation. Use simplified kinematic client prediction.

Serialization & Wire Protocol
- rkyv for snapshot encoding (fast zero-copy). Add a version header and feature flags for forward-compatibility.
- Inputs should be compact: bit-packed control flags, thrust, rotation delta, fire bits, seq id.
- Bundle multiple inputs per packet where possible.
- Provide protocol version negotiation at connection time.

Networking Constants (sensible defaults)
- Server sim tick: 30 Hz
- Input rate: 30 TPS
- Snapshot rate: 12 Hz
- Interp buffer: 120 ms
- Max players per lobby: 10
- Default arena: 5000 × 5000 units
- Wave length: 120s, boss every 3 waves, shop 20s between waves
- Respawn: 5s respawn timer, -10% points penalty on death
- Rejoin grace: 120s

Persistence, Scaling & Deployment
- Single-process initial deployment: redb (embedded) for profiles, in-memory matches per process
- Observability: tracing + metrics (Prometheus), structured logs (JSON), and per-lobby performance histograms (tick times, snapshot size)
- Scaling: shard rooms across processes; consider Redis or Kafka for cross-process events if migrating to a multi-server architecture; switch to Postgres for long-term storage.

Anti-Cheat & Validation
- Server validation for resource changes (ammo/fuel), fire rate, and authoritative position corrections
- Rate-limits per-connection, heartbeat timeouts, and input sanity checks
- Log and metric suspicious behavior (e.g., too many corrections, impossible movements)

Tooling & Debug Support
- Client debug overlay: colliders, reconciliation deltas, netgraph, snapshot age
- Server tooling: deterministic input recorder/replayer for replay-based debugging and regression tests; bot clients for soak tests
- CI: unit tests for critical systems, deterministic regression tests using recorded inputs

Sensible Defaults (quick reference)
- Sim: 30 Hz
- Inputs: 30 TPS
- Snapshots: 12 Hz
- Interp buffer: 120 ms
- Arena: 5000 × 5000 units
- Wave: 120s; Boss every 3 waves; shop 20s
- Max players: 10
- Respawn: 5s
- Rejoin grace: 120s
- Default drop rates: ammo 20%, fuel 15%, secondary 5%, credits 30%

Open Questions (to finalize)
- World model: bounded arena
- Movement style: thrust+rotate (Asteroids)
- Boss cadence: use wave-based boss cadence with a boss every 5 waves
- Death model: configurable timed respawn with a default of 30 seconds.
- In-match vs meta progression: how much progression should persist between matches at launch? Purchases of new ships, as well as weapons and shielf levels should persist as part of a player's game profile so they retain these upgrades across games and game server restarts.
- Secondary weapons: Initially we should support only two kinds of secondary weapons. A "shot-gun" which offers a single burst of projectiles in all directions each time its fired with a max ammo storage of 10 such shots. We should also support a mini-nuke which is a single projectile that detonates on impact or when the player presses "p" key after firing the projectile. When it detonates, it destroyes everything in a radius that is proportional to the level of the weapon. A good starting point is a radius that is similar to the size of the player's ship.
- Friendly fire: off by default 
- Matchmaking: only private rooms to start with.
- Persistence approach and auth: Players can create accounts simply by providing their name, we can forgo any more meaningful authentication for now. For example, if I join the server as "BoneCrusher" for the first time it will create my profile automatically so it can retain my stats and upgrades across games and restarts of the game server. The next time I join the server, if I use the same name (BoneCrusher) all my stats and persistent upgrades are recalled from persistence.
- Netcode constants: lets go with the suggested defaults above.
