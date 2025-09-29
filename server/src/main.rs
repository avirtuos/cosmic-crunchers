use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{sync::broadcast, time};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, warn};
use uuid::Uuid;

mod components;
mod simulation;
mod wire_format;

use components::InputData;
use rapier2d::prelude::Vector;
use simulation::GameSimulation;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomCode(String);

impl RoomCode {
    pub fn generate() -> Self {
        // Generate 8-character alphanumeric room code
        let mut code = String::new();
        for _ in 0..8 {
            let char = match rand::random::<u8>() % 36 {
                0..=9 => (b'0' + rand::random::<u8>() % 10) as char,
                _ => (b'A' + rand::random::<u8>() % 26) as char,
            };
            code.push(char);
        }
        Self(code)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    Join {
        room_code: String,
        player_name: String,
    },
    Input {
        sequence: u32,
        timestamp: u64,
        data: Vec<u8>,
    },
    Ping {
        timestamp: u64,
    },
    RequestDebugRender {
        timestamp: u64,
    },
    Leave,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    RoomJoined {
        room_code: String,
        player_id: String,
        entity_id: u64, // Add the hecs entity ID
    },
    RoomCreated {
        room_code: String,
    },
    PlayerJoined {
        player_id: String,
        player_name: String,
    },
    PlayerLeft {
        player_id: String,
    },
    Snapshot {
        sequence: u32,
        timestamp: u64,
        data: Vec<u8>,
    },
    Pong {
        timestamp: u64,
    },
    DebugRender {
        sequence: u32,
        timestamp: u64,
        data: Vec<u8>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug)]
pub struct Player {
    pub id: Uuid,
    pub name: String,
    pub last_seen: Instant,
    pub sender: broadcast::Sender<ServerMessage>,
}

pub struct Room {
    pub code: RoomCode,
    pub players: HashMap<Uuid, Player>,
    pub created_at: Instant,
    pub last_activity: Instant,
    pub simulation: GameSimulation,
    pub player_entities: HashMap<Uuid, hecs::Entity>, // Map player IDs to their ship entities
}

impl Default for Room {
    fn default() -> Self {
        Self::new()
    }
}

impl Room {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            code: RoomCode::generate(),
            players: HashMap::new(),
            created_at: now,
            last_activity: now,
            simulation: GameSimulation::new(),
            player_entities: HashMap::new(),
        }
    }

    pub fn add_player(&mut self, player: Player) -> Result<(), String> {
        if self.players.len() >= 10 {
            return Err("Room is full".to_string());
        }

        // Spawn ship entity in simulation
        let spawn_position = self.get_spawn_position();
        let ship_entity =
            self.simulation
                .spawn_player_ship(player.id, player.name.clone(), spawn_position);

        // Map player to their ship entity
        self.player_entities.insert(player.id, ship_entity);

        // Notify existing players
        let join_msg = ServerMessage::PlayerJoined {
            player_id: player.id.to_string(),
            player_name: player.name.clone(),
        };

        for existing_player in self.players.values() {
            let _ = existing_player.sender.send(join_msg.clone());
        }

        self.players.insert(player.id, player);
        self.last_activity = Instant::now();
        Ok(())
    }

    fn get_spawn_position(&self) -> Vector<f32> {
        // Simple spawn positioning - spread players around the center
        let player_count = self.players.len() as f32;
        let angle = player_count * 2.0 * std::f32::consts::PI / 8.0; // Up to 8 positions
        let radius = 100.0;
        Vector::new(angle.cos() * radius, angle.sin() * radius)
    }

    pub fn remove_player(&mut self, player_id: Uuid) {
        if self.players.remove(&player_id).is_some() {
            // Remove ship entity from simulation
            if let Some(ship_entity) = self.player_entities.remove(&player_id) {
                self.simulation.despawn_entity(ship_entity);
            }

            let leave_msg = ServerMessage::PlayerLeft {
                player_id: player_id.to_string(),
            };

            for existing_player in self.players.values() {
                let _ = existing_player.sender.send(leave_msg.clone());
            }

            self.last_activity = Instant::now();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    pub fn cleanup_inactive_players(&mut self) {
        let cutoff = Instant::now() - Duration::from_secs(120); // 2 minutes timeout
        let inactive_players: Vec<Uuid> = self
            .players
            .iter()
            .filter(|(_, player)| player.last_seen < cutoff)
            .map(|(id, _)| *id)
            .collect();

        for player_id in inactive_players {
            warn!("Removing inactive player: {}", player_id);
            self.remove_player(player_id);
        }
    }
}

pub type SharedRooms = Arc<Mutex<HashMap<String, Room>>>;

#[derive(Clone)]
pub struct AppState {
    pub rooms: SharedRooms,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Get configuration from environment variables
    let server_host =
        std::env::var("COSMIC_SERVER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let server_port = std::env::var("COSMIC_SERVER_PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .expect("COSMIC_SERVER_PORT must be a valid port number");

    // Build client URL for CORS (assume same host as client, default port 5173)
    let client_host = std::env::var("CLIENT_HOST").unwrap_or_else(|_| "localhost".to_string());
    let client_port = std::env::var("CLIENT_PORT").unwrap_or_else(|_| "5173".to_string());
    let client_url = format!("http://{}:{}", client_host, client_port);

    let state = AppState {
        rooms: Arc::new(Mutex::new(HashMap::new())),
    };

    // Start room cleanup task
    let cleanup_rooms = state.rooms.clone();
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            cleanup_rooms_task(&cleanup_rooms).await;
        }
    });

    let app = Router::new()
        .route("/", get(|| async { "Cosmic Crunchers Server" }))
        .route("/ws", get(websocket_handler))
        .route("/create-room", post(create_room))
        .route("/rooms", get(list_rooms))
        .layer(
            CorsLayer::new()
                .allow_origin(client_url.parse::<axum::http::HeaderValue>().unwrap())
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    let bind_address = format!("{}:{}", server_host, server_port);
    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("Failed to bind to address");

    info!(
        "Cosmic Crunchers server listening on http://{}",
        bind_address
    );
    info!("WebSocket endpoint: ws://{}/ws", bind_address);
    info!("CORS configured for: {}", client_url);

    axum::serve(listener, app)
        .await
        .expect("Failed to start server");
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let player_id = Uuid::new_v4();
    let (tx, mut rx) = broadcast::channel(100);

    info!("New WebSocket connection: {}", player_id);

    // Task to send messages to client
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg)
                && sender.send(Message::Text(json.into())).await.is_err()
            {
                break;
            }
        }
    });

    // Task to receive messages from client
    let recv_task = {
        let state = state.clone();
        tokio::spawn(async move {
            let mut current_room: Option<String> = None;

            while let Some(msg) = receiver.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                            match client_msg {
                                ClientMessage::Join {
                                    room_code,
                                    player_name,
                                } => {
                                    current_room = handle_join(
                                        &state,
                                        &room_code,
                                        &player_name,
                                        player_id,
                                        tx.clone(),
                                    )
                                    .await;
                                }
                                ClientMessage::Input {
                                    sequence,
                                    timestamp,
                                    data,
                                } => {
                                    // Parse input data and add to simulation
                                    if let Some(room_code) = &current_room {
                                        handle_input(
                                            &state, room_code, player_id, sequence, timestamp, data,
                                        )
                                        .await;
                                    }
                                }
                                ClientMessage::Ping { timestamp } => {
                                    let pong = ServerMessage::Pong { timestamp };
                                    let _ = tx.send(pong);

                                    // Update last seen time
                                    if let Some(room_code) = &current_room {
                                        update_player_activity(&state, room_code, player_id).await;
                                    }
                                }
                                ClientMessage::RequestDebugRender { timestamp } => {
                                    // Handle debug render request
                                    if let Some(room_code) = &current_room {
                                        handle_debug_request(
                                            &state, room_code, player_id, timestamp,
                                        )
                                        .await;
                                    }
                                }
                                ClientMessage::Leave => {
                                    if let Some(room_code) = &current_room {
                                        leave_room(&state, room_code, player_id).await;
                                    }
                                    break;
                                }
                            }
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }

            // Clean up on disconnect
            if let Some(room_code) = current_room {
                leave_room(&state, &room_code, player_id).await;
            }
        })
    };

    // Wait for either task to complete
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    info!("WebSocket connection closed: {}", player_id);
}

async fn handle_join(
    state: &AppState,
    room_code: &str,
    player_name: &str,
    player_id: Uuid,
    sender: broadcast::Sender<ServerMessage>,
) -> Option<String> {
    let mut rooms = state.rooms.lock().unwrap();

    if let Some(room) = rooms.get_mut(room_code) {
        let player = Player {
            id: player_id,
            name: player_name.to_string(),
            last_seen: Instant::now(),
            sender: sender.clone(),
        };

        match room.add_player(player) {
            Ok(()) => {
                // Get the entity ID for this player
                let entity_id = room
                    .player_entities
                    .get(&player_id)
                    .map(|entity| entity.id() as u64)
                    .unwrap_or(0);

                let join_msg = ServerMessage::RoomJoined {
                    room_code: room_code.to_string(),
                    player_id: player_id.to_string(),
                    entity_id,
                };
                let _ = rooms
                    .get(room_code)
                    .unwrap()
                    .players
                    .get(&player_id)
                    .unwrap()
                    .sender
                    .send(join_msg);
                info!("Player {} joined room {}", player_name, room_code);
                Some(room_code.to_string())
            }
            Err(e) => {
                let error_msg = ServerMessage::Error { message: e };
                let _ = sender.send(error_msg);
                None
            }
        }
    } else {
        let error_msg = ServerMessage::Error {
            message: "Room not found".to_string(),
        };
        let _ = sender.send(error_msg);
        None
    }
}

async fn leave_room(state: &AppState, room_code: &str, player_id: Uuid) {
    let mut rooms = state.rooms.lock().unwrap();

    if let Some(room) = rooms.get_mut(room_code) {
        room.remove_player(player_id);
        info!("Player {} left room {}", player_id, room_code);

        // Remove empty rooms
        if room.is_empty() {
            rooms.remove(room_code);
            info!("Removed empty room: {}", room_code);
        }
    }
}

async fn update_player_activity(state: &AppState, room_code: &str, player_id: Uuid) {
    let mut rooms = state.rooms.lock().unwrap();

    if let Some(room) = rooms.get_mut(room_code)
        && let Some(player) = room.players.get_mut(&player_id)
    {
        player.last_seen = Instant::now();
    }
}

async fn create_room(State(state): State<AppState>) -> impl IntoResponse {
    let mut rooms = state.rooms.lock().unwrap();
    let room = Room::new();
    let room_code = room.code.as_str().to_string();

    rooms.insert(room_code.clone(), room);
    info!("Created new room: {}", room_code);

    // Start simulation loop for this room
    let simulation_rooms = state.rooms.clone();
    let simulation_room_code = room_code.clone();
    tokio::spawn(async move {
        run_room_simulation(simulation_rooms, simulation_room_code).await;
    });

    (StatusCode::CREATED, room_code)
}

async fn run_room_simulation(rooms: SharedRooms, room_code: String) {
    let mut interval = time::interval(Duration::from_millis(67)); // ~15 Hz

    loop {
        interval.tick().await;

        let mut rooms_guard = rooms.lock().unwrap();
        if let Some(room) = rooms_guard.get_mut(&room_code) {
            // Skip if room is empty
            if room.is_empty() {
                continue;
            }

            // Step simulation
            let step_result = room.simulation.step(1.0 / 15.0); // 15 Hz timestep

            // Send snapshot if generated
            if let Some(snapshot) = step_result.snapshot {
                let snapshot_data = if let Ok(json) = serde_json::to_vec(&snapshot) {
                    json
                } else {
                    continue;
                };

                let snapshot_msg = ServerMessage::Snapshot {
                    sequence: snapshot.sequence,
                    timestamp: snapshot.timestamp,
                    data: snapshot_data,
                };

                // Broadcast to all players in the room
                for player in room.players.values() {
                    let _ = player.sender.send(snapshot_msg.clone());
                }
            }
        } else {
            // Room doesn't exist anymore, stop the loop
            info!("Stopping simulation for room {}: room removed", room_code);
            break;
        }
    }
}

async fn list_rooms(State(state): State<AppState>) -> impl IntoResponse {
    let rooms = state.rooms.lock().unwrap();
    let room_list: Vec<serde_json::Value> = rooms
        .iter()
        .map(|(code, room)| {
            serde_json::json!({
                "code": code,
                "players": room.players.len(),
                "created_at": room.created_at.elapsed().as_secs()
            })
        })
        .collect();

    serde_json::to_string(&room_list).unwrap_or_else(|_| "[]".to_string())
}

async fn handle_input(
    state: &AppState,
    room_code: &str,
    player_id: Uuid,
    sequence: u32,
    timestamp: u64,
    data: Vec<u8>,
) {
    info!(
        "🎮 Received input message: player={}, sequence={}, timestamp={}, data_len={}",
        player_id,
        sequence,
        timestamp,
        data.len()
    );

    let mut rooms = state.rooms.lock().unwrap();
    if let Some(room) = rooms.get_mut(room_code) {
        // Parse input data from client
        match serde_json::from_slice::<InputData>(&data) {
            Ok(input_data) => {
                info!(
                    "✅ Successfully parsed input: thrust={}, turn={}, primary_fire={}, secondary_fire={}",
                    input_data.thrust,
                    input_data.turn,
                    input_data.primary_fire,
                    input_data.secondary_fire
                );

                // Add input to simulation
                room.simulation.add_player_input(player_id, input_data);
                info!("📨 Input added to simulation for player {}", player_id);
            }
            Err(e) => {
                warn!(
                    "❌ Failed to parse input data for player {}: {}",
                    player_id, e
                );
                warn!("Raw data: {:?}", String::from_utf8_lossy(&data));
            }
        }
    } else {
        warn!(
            "❌ Room {} not found for input from player {}",
            room_code, player_id
        );
    }
}

async fn handle_debug_request(state: &AppState, room_code: &str, player_id: Uuid, timestamp: u64) {
    info!(
        "🔍 Received debug render request from player {} in room {}",
        player_id, room_code
    );

    let mut rooms = state.rooms.lock().unwrap();
    if let Some(room) = rooms.get_mut(room_code) {
        // Generate debug render data from the simulation
        let debug_data = room.simulation.generate_debug_render_data();

        // Serialize debug data to JSON bytes
        match serde_json::to_vec(&debug_data) {
            Ok(debug_bytes) => {
                let debug_msg = ServerMessage::DebugRender {
                    sequence: debug_data.sequence,
                    timestamp,
                    data: debug_bytes,
                };

                // Send debug data to the requesting player
                if let Some(player) = room.players.get(&player_id) {
                    match player.sender.send(debug_msg) {
                        Ok(_) => {
                            info!(
                                "✅ Sent debug render data to player {} (sequence: {}, bodies: {}, colliders: {})",
                                player_id,
                                debug_data.sequence,
                                debug_data.rigid_bodies.len(),
                                debug_data.colliders.len()
                            );
                        }
                        Err(e) => {
                            warn!(
                                "❌ Failed to send debug render data to player {}: {}",
                                player_id, e
                            );
                        }
                    }
                } else {
                    warn!(
                        "❌ Player {} not found in room {} for debug request",
                        player_id, room_code
                    );
                }
            }
            Err(e) => {
                warn!(
                    "❌ Failed to serialize debug render data for player {}: {}",
                    player_id, e
                );
            }
        }
    } else {
        warn!(
            "❌ Room {} not found for debug request from player {}",
            room_code, player_id
        );
    }
}

async fn cleanup_rooms_task(rooms: &SharedRooms) {
    let mut rooms_guard = rooms.lock().unwrap();
    let mut rooms_to_remove = Vec::new();

    for (room_code, room) in rooms_guard.iter_mut() {
        room.cleanup_inactive_players();

        // Remove rooms that have been empty for more than 5 minutes
        if room.is_empty() && room.last_activity.elapsed() > Duration::from_secs(300) {
            rooms_to_remove.push(room_code.clone());
        }
    }

    for room_code in rooms_to_remove {
        rooms_guard.remove(&room_code);
        info!("Cleaned up empty room: {}", room_code);
    }
}
