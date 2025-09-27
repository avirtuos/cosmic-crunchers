//! Game simulation loop and systems
//!
//! This module contains the core game simulation logic, including:
//! - Fixed timestep simulation loop (30 Hz)
//! - ECS systems for movement, physics, and game logic
//! - Integration with Rapier2D physics
//! - Snapshot generation for networking

#![allow(dead_code)] // Allow unused code during Phase 2 infrastructure development

use crate::components::*;
use hecs::{Entity, World};
use rapier2d::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Target simulation frequency (30 Hz)
const SIM_TICK_RATE: f32 = 30.0;
const SIM_TICK_DURATION: Duration = Duration::from_nanos((1_000_000_000.0 / SIM_TICK_RATE) as u64);

/// Snapshot frequency (12 Hz by default)
const SNAPSHOT_RATE: f32 = 12.0;
const SNAPSHOTS_PER_TICK: u32 = (SIM_TICK_RATE / SNAPSHOT_RATE) as u32;

/// Game simulation state for a single room
pub struct GameSimulation {
    /// ECS World containing all entities and components
    pub world: World,

    /// Physics world for collision detection and response
    pub physics: PhysicsWorld,

    /// Mapping from ECS entities to physics bodies
    pub entity_to_body: HashMap<Entity, RigidBodyHandle>,

    /// Mapping from physics bodies to ECS entities
    pub body_to_entity: HashMap<RigidBodyHandle, Entity>,

    /// Current simulation tick
    pub tick: u64,

    /// Accumulator for fixed timestep
    pub accumulator: Duration,

    /// Last update time
    pub last_update: Instant,

    /// Snapshot sequence number
    pub snapshot_sequence: u32,

    /// Input recording for deterministic replay
    pub input_recorder: Option<InputRecorder>,

    /// Room bounds for containment
    pub bounds: GameBounds,
}

/// Physics world wrapper
pub struct PhysicsWorld {
    pub rigid_body_set: RigidBodySet,
    pub collider_set: ColliderSet,
    pub gravity: Vector<f32>,
    pub integration_parameters: IntegrationParameters,
    pub physics_pipeline: PhysicsPipeline,
    pub island_manager: IslandManager,
    pub broad_phase: BroadPhaseBvh,
    pub narrow_phase: NarrowPhase,
    pub impulse_joint_set: ImpulseJointSet,
    pub multibody_joint_set: MultibodyJointSet,
    pub ccd_solver: CCDSolver,
    pub physics_hooks: (),
    pub event_handler: CollisionEventCollector,
}

impl Default for PhysicsWorld {
    fn default() -> Self {
        Self {
            rigid_body_set: RigidBodySet::new(),
            collider_set: ColliderSet::new(),
            gravity: vector![0.0, 0.0], // No gravity in space
            integration_parameters: IntegrationParameters::default(),
            physics_pipeline: PhysicsPipeline::new(),
            island_manager: IslandManager::new(),
            broad_phase: BroadPhaseBvh::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joint_set: ImpulseJointSet::new(),
            multibody_joint_set: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
            physics_hooks: (),
            event_handler: CollisionEventCollector::default(),
        }
    }
}

/// Game world boundaries
#[derive(Debug, Clone)]
pub struct GameBounds {
    pub width: f32,
    pub height: f32,
    pub center: Vector<f32>,
}

impl Default for GameBounds {
    fn default() -> Self {
        Self {
            width: 1920.0,
            height: 1080.0,
            center: vector![0.0, 0.0],
        }
    }
}

/// Event collector for physics collisions
#[derive(Default)]
pub struct CollisionEventCollector {
    pub collision_events: VecDeque<CollisionEvent>,
    pub contact_force_events: VecDeque<ContactForceEvent>,
}

impl EventHandler for CollisionEventCollector {
    fn handle_collision_event(
        &self,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        _event: CollisionEvent,
        _contact_pair: Option<&ContactPair>,
    ) {
        // We'll process these in the simulation loop
        // For now, just collect them
    }

    fn handle_contact_force_event(
        &self,
        _dt: f32,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        _contact_pair: &ContactPair,
        _total_force_magnitude: f32,
    ) {
        // Contact force events for advanced physics feedback
    }
}

/// Input recording for deterministic replay
#[derive(Debug)]
pub struct InputRecorder {
    pub recorded_inputs: Vec<RecordedInput>,
    pub is_recording: bool,
}

#[derive(Debug, Clone)]
pub struct RecordedInput {
    pub tick: u64,
    pub player_id: Uuid,
    pub input: InputData,
}

impl GameSimulation {
    pub fn new() -> Self {
        let world = World::new();
        let physics = PhysicsWorld::default();

        Self {
            world,
            physics,
            entity_to_body: HashMap::new(),
            body_to_entity: HashMap::new(),
            tick: 0,
            accumulator: Duration::ZERO,
            last_update: Instant::now(),
            snapshot_sequence: 0,
            input_recorder: None,
            bounds: GameBounds::default(),
        }
    }

    pub fn enable_recording(&mut self) {
        self.input_recorder = Some(InputRecorder {
            recorded_inputs: Vec::new(),
            is_recording: true,
        });
        info!("Input recording enabled for simulation");
    }

    pub fn disable_recording(&mut self) -> Option<Vec<RecordedInput>> {
        if let Some(recorder) = self.input_recorder.take() {
            info!(
                "Input recording disabled, collected {} inputs",
                recorder.recorded_inputs.len()
            );
            Some(recorder.recorded_inputs)
        } else {
            None
        }
    }

    /// Create a player ship entity
    pub fn spawn_player_ship(
        &mut self,
        player_id: Uuid,
        name: String,
        spawn_position: Vector<f32>,
    ) -> Entity {
        // Create physics body
        let rigid_body = RigidBodyBuilder::dynamic()
            .translation(spawn_position)
            .linear_damping(0.5) // Space friction
            .angular_damping(2.0)
            .build();

        let body_handle = self.physics.rigid_body_set.insert(rigid_body);

        // Create collider
        let collider = ColliderBuilder::ball(8.0) // Ship radius
            .density(1.0)
            .friction(0.0)
            .restitution(0.8)
            .build();

        let collider_handle = self.physics.collider_set.insert_with_parent(
            collider,
            body_handle,
            &mut self.physics.rigid_body_set,
        );

        // Create ECS entity with components
        let entity = self.world.spawn((
            Transform::from_vector(spawn_position, 0.0),
            Velocity::default(),
            Health::default(),
            Player {
                id: player_id,
                name,
                score: 0,
                kills: 0,
                deaths: 0,
                credits: 0,
            },
            InputBuffer::default(),
            Ship::default(),
            crate::components::RigidBody {
                handle: body_handle,
            },
            crate::components::Collider {
                handle: collider_handle,
            },
        ));

        // Map entity to physics body
        self.entity_to_body.insert(entity, body_handle);
        self.body_to_entity.insert(body_handle, entity);

        info!("Spawned player ship for {}: entity={:?}", player_id, entity);
        entity
    }

    /// Remove a player ship entity
    pub fn despawn_entity(&mut self, entity: Entity) {
        // Remove physics body if it exists
        if let Ok(rigid_body) = self.world.get::<&crate::components::RigidBody>(entity) {
            let body_handle = rigid_body.handle;

            // Remove from physics world
            self.physics.rigid_body_set.remove(
                body_handle,
                &mut self.physics.island_manager,
                &mut self.physics.collider_set,
                &mut self.physics.impulse_joint_set,
                &mut self.physics.multibody_joint_set,
                true, // wake_up connected bodies
            );

            // Remove mappings
            self.entity_to_body.remove(&entity);
            self.body_to_entity.remove(&body_handle);
        }

        // Remove ECS entity
        if let Err(e) = self.world.despawn(entity) {
            warn!("Failed to despawn entity {:?}: {}", entity, e);
        } else {
            debug!("Despawned entity: {:?}", entity);
        }
    }

    /// Add input for a specific player
    pub fn add_player_input(&mut self, player_id: Uuid, input: InputData) {
        // Record input if recording is enabled
        if let Some(recorder) = &mut self.input_recorder
            && recorder.is_recording
        {
            recorder.recorded_inputs.push(RecordedInput {
                tick: self.tick,
                player_id,
                input: input.clone(),
            });
        }

        // Find the player's entity and add input to their buffer
        for (_, (player, input_buffer)) in self.world.query_mut::<(&Player, &mut InputBuffer)>() {
            if player.id == player_id {
                input_buffer.add_input(input);
                break;
            }
        }
    }

    /// Step the simulation forward by one tick
    pub fn step(&mut self, dt: f32) -> SimulationStepResult {
        let step_start = Instant::now();

        // Process inputs
        self.process_inputs(dt);

        // Update movement and apply forces
        self.update_movement(dt);

        // Step physics simulation
        self.step_physics(dt);

        // Sync physics back to ECS
        self.sync_physics_to_ecs();

        // Update game logic systems
        self.update_lifetime_system(dt);
        self.update_health_system(dt);

        // Apply boundary constraints
        self.apply_boundaries();

        // Advance tick
        self.tick += 1;

        // Generate snapshot if needed
        let snapshot = if self.tick.is_multiple_of(SNAPSHOTS_PER_TICK as u64) {
            self.snapshot_sequence += 1;
            Some(self.generate_snapshot())
        } else {
            None
        };

        let step_duration = step_start.elapsed();

        SimulationStepResult {
            tick: self.tick,
            step_duration,
            entity_count: self.world.len(),
            snapshot,
        }
    }

    /// Process all pending inputs for this tick
    fn process_inputs(&mut self, _dt: f32) {
        for (_, (_player, input_buffer, _ship)) in
            self.world.query_mut::<(&Player, &mut InputBuffer, &Ship)>()
        {
            // Process all available inputs for this player
            while let Some(input) = input_buffer.get_next_input() {
                // Apply thrust
                if input.thrust > 0.0 {
                    // We'll apply forces in update_movement
                }

                // Apply turning
                if input.turn != 0.0 {
                    // Angular velocity will be applied in update_movement
                }

                // Note: weapon firing will be handled in weapon systems (Phase 4)
            }
        }
    }

    /// Update movement forces and velocities
    fn update_movement(&mut self, _dt: f32) {
        // Apply input-based forces to physics bodies
        for (entity, (transform, ship, input_buffer)) in
            self.world
                .query_mut::<(&mut Transform, &Ship, &InputBuffer)>()
        {
            if let Some(body_handle) = self.entity_to_body.get(&entity)
                && let Some(body) = self.physics.rigid_body_set.get_mut(*body_handle) {
                // Get the most recent input if available
                if let Some(latest_input) = input_buffer.buffer.back() {
                    // Apply thrust force
                    if latest_input.thrust > 0.0 {
                        let thrust_direction =
                            Vector::new(transform.rotation.cos(), transform.rotation.sin());
                        let thrust_force =
                            thrust_direction * ship.thrust_power * latest_input.thrust;
                        body.add_force(thrust_force, true);
                    }

                    // Apply turning torque
                    if latest_input.turn != 0.0 {
                        let torque = -latest_input.turn * ship.turn_rate * ship.mass * 100.0;
                        body.add_torque(torque, true);
                    }
                }
            }
        }
    }

    /// Step the physics simulation
    fn step_physics(&mut self, dt: f32) {
        self.physics.integration_parameters.dt = dt;

        self.physics.physics_pipeline.step(
            &self.physics.gravity,
            &self.physics.integration_parameters,
            &mut self.physics.island_manager,
            &mut self.physics.broad_phase,
            &mut self.physics.narrow_phase,
            &mut self.physics.rigid_body_set,
            &mut self.physics.collider_set,
            &mut self.physics.impulse_joint_set,
            &mut self.physics.multibody_joint_set,
            &mut self.physics.ccd_solver,
            &self.physics.physics_hooks,
            &self.physics.event_handler,
        );
    }

    /// Sync physics body positions back to ECS transform components
    fn sync_physics_to_ecs(&mut self) {
        for (entity, transform) in self.world.query_mut::<&mut Transform>() {
            if let Some(body_handle) = self.entity_to_body.get(&entity)
                && let Some(body) = self.physics.rigid_body_set.get(*body_handle) {
                let position = body.translation();
                let rotation = body.rotation().angle();

                transform.position = [position.x, position.y];
                transform.rotation = rotation;
            }
        }
    }

    /// Update lifetime system for temporary entities
    fn update_lifetime_system(&mut self, dt: f32) {
        let mut entities_to_remove = Vec::new();

        for (entity, lifetime) in self.world.query_mut::<&mut Lifetime>() {
            lifetime.remaining -= dt;
            if lifetime.remaining <= 0.0 {
                entities_to_remove.push(entity);
            }
        }

        for entity in entities_to_remove {
            self.despawn_entity(entity);
        }
    }

    /// Update health system (shield regeneration, etc.)
    fn update_health_system(&mut self, dt: f32) {
        let current_time = self.tick as f64 * (1.0 / SIM_TICK_RATE as f64);

        for (_, health) in self.world.query_mut::<&mut Health>() {
            // Shield regeneration
            if current_time - health.last_damage_time >= health.shield_recharge_delay as f64
                && health.shield < health.shield_max {
                health.shield += health.shield_recharge_rate * dt;
                health.shield = health.shield.min(health.shield_max);
            }
        }
    }

    /// Apply boundary constraints to keep entities in bounds
    fn apply_boundaries(&mut self) {
        let bounds = self.bounds.clone();

        for (entity, transform) in self.world.query_mut::<&mut Transform>() {
            let half_width = bounds.width / 2.0;
            let half_height = bounds.height / 2.0;

            let mut position = transform.position;
            let mut clamped = false;

            if position[0] < bounds.center.x - half_width {
                position[0] = bounds.center.x - half_width;
                clamped = true;
            } else if position[0] > bounds.center.x + half_width {
                position[0] = bounds.center.x + half_width;
                clamped = true;
            }

            if position[1] < bounds.center.y - half_height {
                position[1] = bounds.center.y - half_height;
                clamped = true;
            } else if position[1] > bounds.center.y + half_height {
                position[1] = bounds.center.y + half_height;
                clamped = true;
            }

            if clamped {
                transform.position = position;
                // Also update physics body if it exists
                if let Some(body_handle) = self.entity_to_body.get(&entity)
                    && let Some(body) = self.physics.rigid_body_set.get_mut(*body_handle) {
                    let pos_vector = Vector::new(position[0], position[1]);
                    body.set_translation(pos_vector, true);
                    // Reduce velocity when hitting boundaries
                    let velocity = body.linvel() * 0.5;
                    body.set_linvel(velocity, true);
                }
            }
        }
    }

    /// Generate a snapshot of the current game state
    fn generate_snapshot(&self) -> GameSnapshot {
        let mut entities = Vec::new();

        for (entity, (transform, player)) in self.world.query::<(&Transform, &Player)>().iter() {
            let health = self
                .world
                .get::<&Health>(entity)
                .map(|h| (*h).clone())
                .unwrap_or_else(|_| Health::default());
            let velocity = self
                .world
                .get::<&Velocity>(entity)
                .map(|v| (*v).clone())
                .unwrap_or_else(|_| Velocity::default());

            entities.push(EntitySnapshot {
                entity_id: entity.id() as u64, // Convert hecs::Entity to u64
                entity_type: EntityType::Player(player.clone()),
                transform: transform.clone(),
                velocity,
                health,
            });
        }

        GameSnapshot {
            sequence: self.snapshot_sequence,
            tick: self.tick,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            entities,
        }
    }
}

/// Result of a simulation step
#[derive(Debug)]
pub struct SimulationStepResult {
    pub tick: u64,
    pub step_duration: Duration,
    pub entity_count: u32,
    pub snapshot: Option<GameSnapshot>,
}

/// Network-serializable snapshot of game state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSnapshot {
    pub sequence: u32,
    pub tick: u64,
    pub timestamp: u64,
    pub entities: Vec<EntitySnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySnapshot {
    pub entity_id: u64,
    pub entity_type: EntityType,
    pub transform: Transform,
    pub velocity: Velocity,
    pub health: Health,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EntityType {
    Player(Player),
    Projectile(Projectile),
    Enemy, // Will be expanded in Phase 4
}
