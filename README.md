# Cosmic Crunchers

A multiplayer, browser-based 2D space shooter where friends cooperate to destroy waves of enemies and asteroids while collecting resources and upgrades. Built with Rust (server) and TypeScript + Phaser 3 (client).

## Quick Start

### Prerequisites

Run the environment setup script to install required tools:

```bash
chmod +x ./dev_setup.sh
./dev_setup.sh
```

This script will check for and optionally install:
- Basic tools: git, curl, wget, jq, make, cmake
- Rust toolchain: rustup, cargo, rustfmt, clippy
- Node.js (>= 18) and npm (preferably via nvm)

If Node was installed via nvm, restart your terminal or source your shell profile after the script completes.

### Development

**Start the server:**
```bash
cd server
cargo run
```

**Start the client (in a new terminal):**
```bash
cd client
npm run dev
```

The client will be available at http://localhost:5173/

### Project Structure

```
cosmic-crunchers/
├── server/          # Rust server crate (authoritative game simulation)
├── client/          # TypeScript + Phaser 3 client (rendering, input, UI)
├── docs/            # Design and implementation documentation
├── tools/           # Scripts for testing, bots, record/replay
├── dev_setup.sh     # Development environment setup script
└── README.md        # This file
```

## Architecture Overview

- **Server**: Rust-based authoritative server using axum (WebSocket), tokio (async runtime), hecs (ECS), rapier2d (physics), and rkyv (serialization)
- **Client**: TypeScript + Phaser 3 for rendering, input handling, and UI with WebSocket communication
- **Netcode**: Server-authoritative simulation with client prediction and interpolation
- **Physics**: Rapier2D for collision detection and physics simulation
- **Persistence**: redb for profiles and progression data

## Key Features (Planned)

- **Multiplayer**: Up to 10 players per room with 8-digit room codes
- **Real-time Combat**: Authoritative server simulation at 30 Hz
- **Wave-based Gameplay**: Survive waves of enemies with periodic boss encounters
- **Weapon Systems**: Primary weapons (unlimited ammo) and secondary weapons (limited, powerful)
- **Progression**: In-match shops, persistent unlocks, and player profiles
- **Physics**: Full 2D physics with collision detection and projectile simulation

## Documentation

- [Design Document](docs/design.md) - Complete game design and technical specifications
- [Implementation Roadmap](docs/implementation.md) - Phased development plan with tasks and acceptance criteria

## Development Workflow

1. **Phase 0** (Current): Project scaffolding and CI setup
2. **Phase 1**: Lobby system and basic WebSocket networking
3. **Phase 2**: Server simulation loop with ECS and physics
4. **Phase 3**: Client prototype with prediction and interpolation
5. **Phase 4+**: Weapons, enemies, economy, persistence, and polish

## Building and Testing

**Server:**
```bash
cd server
cargo build          # Build
cargo test           # Run tests
cargo clippy         # Lint
cargo fmt            # Format
```

**Client:**
```bash
cd client
npm run build        # Build for production
npm run preview      # Preview production build
npm run type-check   # TypeScript type checking
```

## Contributing

1. Follow the implementation roadmap in `docs/implementation.md`
2. Ensure all tests pass before submitting changes
3. Use conventional commit messages
4. Run formatters and linters before committing

## License

See [LICENSE](LICENSE) for details.
