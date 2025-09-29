//! Game simulation loop and systems
//!
//! This module contains the core game simulation logic, including:
//! - Fixed timestep simulation loop (30 Hz)
//! - ECS systems for movement, physics, and game logic
//! - Integration with Rapier2D physics
//! - Snapshot generation for networking

#![allow(dead_code)] // Allow unused code during Phase 2 infrastructure development

use crate::components::*;
use crate::wire_format::{
    DebugBodyType, DebugCollider, DebugJoint, DebugJointType, DebugRenderData, DebugRigidBody,
    DebugShape, DebugVelocity,
};
use hecs::{Entity, World};
use rapier2d::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Target simulation frequency (15 Hz) - matches snapshot frequency for simplicity
const SIM_TICK_RATE: f32 = 15.0;
const SIM_TICK_DURATION: Duration = Duration::from_nanos((1_000_000_000.0 / SIM_TICK_RATE) as u64);

/// Snapshot frequency matches simulation frequency (15 Hz)
const SNAPSHOT_RATE: f32 = 15.0;
// No need for SNAPSHOTS_PER_TICK - every simulation tick generates a snapshot

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

impl Default for GameSimulation {
    fn default() -> Self {
        Self::new()
    }
}

impl GameSimulation {
    pub fn new() -> Self {
        let world = World::new();
        let mut physics = PhysicsWorld::default();

        // Configure integration parameters for proper damping behavior
        physics.integration_parameters.dt = 1.0 / SIM_TICK_RATE; // 1/15 = 0.0667 seconds per step
        // Note: Other integration parameters like max_velocity_iterations don't exist in this Rapier version

        debug!(
            "Initialized physics with dt={:.4}s for {}Hz simulation",
            physics.integration_parameters.dt, SIM_TICK_RATE
        );

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
            .linear_damping(0.4) // Realistic damping for smooth gameplay
            .angular_damping(1.0) // Realistic damping for smooth gameplay
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
                name: name.clone(),
                score: 0,
                kills: 0,
                deaths: 0,
                credits: 0,
            },
            InputBuffer::default(),
            Ship::default(),
            Weapon {
                weapon_type: WeaponType::RapidFire {
                    rate: 5.0, // 5 shots per second
                    damage: 25.0,
                    speed: 300.0, // pixels per second
                },
                last_fire_time: 0.0,
                ammo: None,    // Unlimited ammo for now
                cooldown: 0.2, // 200ms cooldown
            },
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

        // Update Ship component with physics-calculated values for consistency
        if let Ok(mut ship) = self.world.get::<&mut Ship>(entity)
            && let Some(body) = self.physics.rigid_body_set.get(body_handle) {
                // Update mass with physics-calculated value
                ship.mass = body.mass();

                // Update size with collider radius (we know it's a ball collider with radius 8.0)
                ship.size = 8.0;

                // Log the final ship configuration that will be sent to client
                info!("🚀 Ship config for Player [{}]:", name);
                info!("  Mass: {:.1}kg | Size: {:.1}px", ship.mass, ship.size);
                info!("  Thrust Power: {:.0}N", ship.thrust_power);
                info!("  Turn Rate: {:.1} rad/s", ship.turn_rate);
                info!("  Max Speed: {:.1} px/s", ship.max_speed);

                info!(
                    "Created new ship for player[{}] with physics mass[{:.1}], linear_damp[{}], angular_damp[{}]. and spawn[{}]",
                    player_id,
                    body.mass(),
                    body.linear_damping(),
                    body.angular_damping(),
                    body.position()
                );
            }

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
        info!(
            "🔧 Adding input to simulation: player={}, thrust={}, turn={}, seq={}",
            player_id, input.thrust, input.turn, input.sequence
        );

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
        let mut found_player = false;
        for (_, (player, input_buffer)) in self.world.query_mut::<(&Player, &mut InputBuffer)>() {
            if player.id == player_id {
                info!("✅ Found matching player in ECS world: {}", player.name);
                input_buffer.add_input(input);
                info!(
                    "📝 Input added to buffer. Buffer size: {}",
                    input_buffer.buffer.len()
                );
                found_player = true;
                break;
            }
        }

        if !found_player {
            warn!(
                "❌ No matching player found in ECS world for player_id: {}",
                player_id
            );
            info!("Available players in ECS:");
            for (_, player) in self.world.query::<&Player>().iter() {
                info!("  - Player: {} (id: {})", player.name, player.id);
            }
        }
    }

    /// Step the simulation forward by one tick
    pub fn step(&mut self, dt: f32) -> SimulationStepResult {
        let step_start = Instant::now();

        // Prepare inputs for processing
        self.prepare_inputs(dt);

        // Update movement and apply forces
        self.update_movement(dt);

        // Step physics simulation
        self.step_physics(dt);

        // Note: Removed manual velocity decay - relying on Rapier's built-in damping (2.3 for 2-second decay)

        // Sync physics back to ECS
        self.sync_physics_to_ecs();

        // Update game logic systems
        self.update_lifetime_system(dt);
        self.update_health_system(dt);

        // Apply boundary constraints
        self.apply_boundaries();

        // Advance tick
        self.tick += 1;

        // Generate snapshot every tick (15 Hz simulation = 15 Hz snapshots)
        self.snapshot_sequence += 1;
        let snapshot = Some(self.generate_snapshot());

        let step_duration = step_start.elapsed();

        SimulationStepResult {
            tick: self.tick,
            step_duration,
            entity_count: self.world.len(),
            snapshot,
        }
    }

    /// Prepare inputs for movement processing (consolidate multiple inputs, discard old ones)
    fn prepare_inputs(&mut self, _dt: f32) {
        for (_, (player, input_buffer, _ship)) in
            self.world.query_mut::<(&Player, &mut InputBuffer, &Ship)>()
        {
            // Process all available inputs for this player and keep the latest
            let mut latest_input: Option<InputData> = None;
            let mut discarded_count = 0;

            while let Some(input) = input_buffer.get_next_input() {
                if latest_input.is_some() {
                    discarded_count += 1; // Count inputs we're discarding (keeping only the latest)
                }
                latest_input = Some(input.clone());
            }

            // Log only when discarding inputs (indicates network issues or processing lag)
            if discarded_count > 0 {
                debug!(
                    "⚠️  Player {} discarded {} older inputs, kept latest",
                    player.name, discarded_count
                );
            }

            // Store the latest input temporarily for movement processing
            if let Some(input) = latest_input {
                input_buffer.buffer.clear(); // Clear buffer first
                input_buffer.buffer.push_back(input); // Store only for this frame
            }
        }
    }

    /// Update movement forces and velocities
    fn update_movement(&mut self, _dt: f32) {
        let current_time = self.tick as f64 * (1.0 / SIM_TICK_RATE as f64);

        // First pass: Process weapon firing BEFORE clearing input buffers
        self.process_weapon_firing(current_time);

        // Second pass: Apply movement forces and clear input buffers
        for (entity, (transform, ship, input_buffer, player)) in
            self.world
                .query_mut::<(&mut Transform, &Ship, &mut InputBuffer, Option<&Player>)>()
        {
            if let Some(body_handle) = self.entity_to_body.get(&entity)
                && let Some(body) = self.physics.rigid_body_set.get_mut(*body_handle)
            {
                // ALWAYS apply forces (including zeros) to ensure Rapier integration runs and applies damping
                let thrust_value = input_buffer
                    .buffer
                    .back()
                    .map(|input| input.thrust)
                    .unwrap_or(0.0);
                let turn_value = input_buffer
                    .buffer
                    .back()
                    .map(|input| input.turn)
                    .unwrap_or(0.0);

                // Calculate and apply thrust force
                let thrust_direction =
                    Vector::new(transform.rotation.cos(), transform.rotation.sin());
                let thrust_force = thrust_direction * ship.thrust_power * thrust_value;

                //We reset the forces every cycle be our thrust model is constant.
                //the thrusters are either on or off, they don't accumulate force.
                body.reset_forces(true);
                body.reset_torques(true);

                // Calculate and apply thrust if none zero
                if thrust_value != 0.0 {
                    body.add_force(thrust_force, true); // ALWAYS applied, even if zero
                }

                // Calculate and apply turning torque if none zero
                let torque = -turn_value * ship.turn_rate * ship.mass * 100.0;
                if turn_value != 0.0 {
                    body.add_torque(torque, true);
                }

                // Log actual physics actions taken
                if let Some(player) = player {
                    if thrust_value != 0.0 {
                        debug!(
                            "⚡ Applying thrust to {}: force=[{:.1}, {:.1}]N, magnitude={:.1}",
                            player.name,
                            thrust_force.x,
                            thrust_force.y,
                            thrust_force.magnitude()
                        );
                    }

                    if turn_value != 0.0 {
                        debug!("🌀 Applying torque to {}: {:.1}N⋅m", player.name, torque);
                    }

                    // Log zero force application for damping (less frequently to avoid spam)
                    if thrust_value == 0.0 || turn_value == 0.0 {
                        debug!(
                            "🛑 Applying zero forces to {} for damping integration",
                            player.name
                        );
                    }
                }

                // CRITICAL: Clear input buffer after applying forces to prevent reapplication
                input_buffer.buffer.clear();
            }
        }
    }

    /// Process weapon firing for all players
    fn process_weapon_firing(&mut self, current_time: f64) {
        let mut projectiles_to_spawn = Vec::new();

        // Check all players for weapon firing
        for (_entity, (transform, player, input_buffer, weapon)) in
            self.world
                .query_mut::<(&Transform, &Player, &InputBuffer, &mut Weapon)>()
        {
            if let Some(latest_input) = input_buffer.buffer.back() {
                // Check primary fire
                if latest_input.primary_fire
                    && current_time - weapon.last_fire_time >= weapon.cooldown as f64
                    && let WeaponType::RapidFire { damage, speed, .. } = weapon.weapon_type
                {
                    // Calculate spawn position (front of ship)
                    let spawn_offset = 15.0; // Spawn projectile in front of ship
                    let spawn_position = Vector::new(
                        transform.position[0] + transform.rotation.cos() * spawn_offset,
                        transform.position[1] + transform.rotation.sin() * spawn_offset,
                    );

                    // Calculate projectile velocity
                    let projectile_velocity = Vector::new(
                        transform.rotation.cos() * speed,
                        transform.rotation.sin() * speed,
                    );

                    projectiles_to_spawn.push((
                        spawn_position,
                        projectile_velocity,
                        damage,
                        player.id,
                    ));

                    weapon.last_fire_time = current_time;
                    debug!("Player {} fired primary weapon", player.name);
                }

                // TODO: Add secondary fire processing here when needed
                if latest_input.secondary_fire {
                    debug!(
                        "Player {} attempted secondary fire (not implemented)",
                        player.name
                    );
                }
            }
        }

        // Spawn all projectiles
        for (position, velocity, damage, owner_id) in projectiles_to_spawn {
            self.spawn_projectile(position, velocity, damage, owner_id);
        }
    }

    /// Spawn a projectile entity
    fn spawn_projectile(
        &mut self,
        position: Vector<f32>,
        velocity: Vector<f32>,
        damage: f32,
        owner_id: Uuid,
    ) {
        // Create physics body for projectile
        let rigid_body = RigidBodyBuilder::kinematic_velocity_based()
            .translation(position)
            .linvel(velocity)
            .build();

        let body_handle = self.physics.rigid_body_set.insert(rigid_body);

        // Create collider for projectile
        let collider = ColliderBuilder::ball(2.0) // Small projectile radius
            .density(0.1)
            .friction(0.0)
            .restitution(0.0)
            .build();

        let collider_handle = self.physics.collider_set.insert_with_parent(
            collider,
            body_handle,
            &mut self.physics.rigid_body_set,
        );

        // Create ECS entity for projectile
        let entity = self.world.spawn((
            Transform::from_vector(position, velocity.y.atan2(velocity.x)),
            Velocity::from_vector(velocity, 0.0),
            Projectile {
                damage,
                lifetime: 3.0, // 3 seconds
                speed: velocity.magnitude(),
                owner_id,
            },
            Lifetime {
                remaining: 3.0, // 3 seconds
            },
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

        debug!(
            "Spawned projectile: entity={:?}, owner={}",
            entity, owner_id
        );
    }

    /// Step the physics simulation
    fn step_physics(&mut self, dt: f32) {
        self.physics.integration_parameters.dt = dt;

        // PHYSICS DIAGNOSTICS: Track velocities before/after physics step
        let mut velocity_before = Vec::new();
        for (body_handle, body) in self.physics.rigid_body_set.iter() {
            let vel = body.linvel();
            if vel.magnitude() > 0.1 {
                velocity_before.push((body_handle, vel.magnitude()));
            }
        }

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

        // PHYSICS DIAGNOSTICS: Compare velocities after physics step
        if !velocity_before.is_empty() && self.tick.is_multiple_of(15) {
            debug!("🔬 PHYSICS STEP DIAGNOSTICS (dt={:.4}s):", dt);
            for (body_handle, vel_before) in velocity_before {
                if let Some(body) = self.physics.rigid_body_set.get(body_handle) {
                    let vel_after = body.linvel().magnitude();
                    let damping = body.linear_damping();
                    let change = vel_after - vel_before;
                    let expected_change = -vel_before * damping * dt;

                    debug!(
                        "  Body {:?}: vel {:.2}→{:.2} px/s (Δ{:+.2}) | damping={:.1} | expected Δ{:+.2}",
                        body_handle.into_raw_parts(),
                        vel_before,
                        vel_after,
                        change,
                        damping,
                        expected_change
                    );
                }
            }
        }
    }

    /// Sync physics body positions and velocities back to ECS components
    fn sync_physics_to_ecs(&mut self) {
        // TEMPORARILY REMOVED velocity thresholding to test if it's interfering with damping

        for (entity, (transform, velocity, player)) in
            self.world
                .query_mut::<(&mut Transform, &mut Velocity, Option<&Player>)>()
        {
            if let Some(body_handle) = self.entity_to_body.get(&entity)
                && let Some(body) = self.physics.rigid_body_set.get_mut(*body_handle)
            {
                // Sync position and rotation
                let position = body.translation();
                let rotation = body.rotation().angle();
                transform.position = [position.x, position.y];
                transform.rotation = rotation;

                // Sync velocities directly from physics (no thresholding)
                let linear_vel = *body.linvel();
                let angular_vel = body.angvel();

                // Debug logging for velocity tracking (only for players, every 15 ticks = 1 second at 15 Hz)
                if let Some(player) = player {
                    if self.tick.is_multiple_of(15)
                        && (linear_vel.magnitude() > 0.1 || angular_vel.abs() > 0.001)
                    {
                        debug!(
                            "🚀 Player {} velocity: linear={:.2} px/s [{:.1}, {:.1}], angular={:.3} rad/s",
                            player.name,
                            linear_vel.magnitude(),
                            linear_vel.x,
                            linear_vel.y,
                            angular_vel
                        );
                        debug!(
                            "🔧 Body damping: linear={:.3}, angular={:.3}",
                            body.linear_damping(),
                            body.angular_damping()
                        );
                    }

                    // Additional debug: Log when velocity should be decaying but isn't
                    if self.tick.is_multiple_of(5) && linear_vel.magnitude() > 10.0 {
                        debug!(
                            "⚠️  Player {} velocity NOT decaying: {:.2} px/s (tick {}) - NO THRESHOLDING",
                            player.name,
                            linear_vel.magnitude(),
                            self.tick
                        );
                    }
                }

                velocity.linear = [linear_vel.x, linear_vel.y];
                velocity.angular = angular_vel;
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
                && health.shield < health.shield_max
            {
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
                    && let Some(body) = self.physics.rigid_body_set.get_mut(*body_handle)
                {
                    let pos_vector = Vector::new(position[0], position[1]);
                    body.set_translation(pos_vector, true);
                    // Reduce velocity when hitting boundaries
                    let velocity = body.linvel() * 0.5;
                    body.set_linvel(velocity, true);
                }
            }
        }
    }

    /// Generate debug rendering data from the physics world
    pub fn generate_debug_render_data(&self) -> DebugRenderData {
        let mut debug_data = DebugRenderData {
            sequence: self.snapshot_sequence,
            rigid_bodies: Vec::new(),
            colliders: Vec::new(),
            forces: Vec::new(),
            velocities: Vec::new(),
            joints: Vec::new(),
        };

        // Extract rigid body debug data
        for (handle, body) in self.physics.rigid_body_set.iter() {
            let body_type = match body.body_type() {
                RigidBodyType::Dynamic => DebugBodyType::Dynamic,
                RigidBodyType::KinematicVelocityBased | RigidBodyType::KinematicPositionBased => {
                    DebugBodyType::Kinematic
                }
                RigidBodyType::Fixed => DebugBodyType::Static,
            };

            debug_data.rigid_bodies.push(DebugRigidBody {
                handle: handle.into_raw_parts().0,
                position: [body.translation().x, body.translation().y],
                rotation: body.rotation().angle(),
                body_type,
                mass: body.mass(),
                linear_damping: body.linear_damping(),
                angular_damping: body.angular_damping(),
            });

            // Add velocity data for dynamic bodies
            let linvel = body.linvel();
            let angvel = body.angvel();
            if linvel.magnitude() > 0.01 || angvel.abs() > 0.01 {
                debug_data.velocities.push(DebugVelocity {
                    body_handle: handle.into_raw_parts().0,
                    linear_velocity: [linvel.x, linvel.y],
                    angular_velocity: angvel,
                });
            }
        }

        // Extract collider debug data
        for (handle, collider) in self.physics.collider_set.iter() {
            let parent_handle = collider
                .parent()
                .unwrap_or(RigidBodyHandle::from_raw_parts(0, 0));

            // Convert Rapier shape to debug shape
            let debug_shape = match collider.shape().shape_type() {
                ShapeType::Ball => {
                    if let Some(ball) = collider.shape().as_ball() {
                        DebugShape::Ball {
                            radius: ball.radius,
                        }
                    } else {
                        continue; // Skip if cast fails
                    }
                }
                ShapeType::Cuboid => {
                    if let Some(cuboid) = collider.shape().as_cuboid() {
                        DebugShape::Cuboid {
                            half_extents: [cuboid.half_extents.x, cuboid.half_extents.y],
                        }
                    } else {
                        continue; // Skip if cast fails
                    }
                }
                ShapeType::Triangle => {
                    if let Some(triangle) = collider.shape().as_triangle() {
                        DebugShape::Triangle {
                            vertices: [
                                [triangle.a.x, triangle.a.y],
                                [triangle.b.x, triangle.b.y],
                                [triangle.c.x, triangle.c.y],
                            ],
                        }
                    } else {
                        continue; // Skip if cast fails
                    }
                }
                ShapeType::ConvexPolygon => {
                    if let Some(polygon) = collider.shape().as_convex_polygon() {
                        let vertices = polygon.points().iter().map(|p| [p.x, p.y]).collect();
                        DebugShape::Polygon { vertices }
                    } else {
                        continue; // Skip if cast fails
                    }
                }
                _ => continue, // Skip unsupported shapes
            };

            let identity = Isometry::identity();
            let collider_pos = collider.position_wrt_parent().unwrap_or(&identity);
            debug_data.colliders.push(DebugCollider {
                handle: handle.into_raw_parts().0,
                parent_body: parent_handle.into_raw_parts().0,
                shape: debug_shape,
                position: [collider_pos.translation.x, collider_pos.translation.y],
                rotation: collider_pos.rotation.angle(),
            });
        }

        // Extract joint debug data (simplified - just report all as fixed for now)
        // In a real implementation, you'd need to inspect the joint data more carefully
        for (handle, joint) in self.physics.impulse_joint_set.iter() {
            let body1_handle = joint.body1.into_raw_parts().0;
            let body2_handle = joint.body2.into_raw_parts().0;

            // Get anchor points (simplified - would need more complex extraction for real anchors)
            let anchor1 = if let Some(body) = self.physics.rigid_body_set.get(joint.body1) {
                [body.translation().x, body.translation().y]
            } else {
                [0.0, 0.0]
            };

            let anchor2 = if let Some(body) = self.physics.rigid_body_set.get(joint.body2) {
                [body.translation().x, body.translation().y]
            } else {
                [0.0, 0.0]
            };

            debug_data.joints.push(DebugJoint {
                handle: handle.into_raw_parts().0,
                body1: body1_handle,
                body2: body2_handle,
                anchor1,
                anchor2,
                joint_type: DebugJointType::Fixed, // Simplified for now
            });
        }

        debug_data
    }

    /// Generate a snapshot of the current game state
    fn generate_snapshot(&self) -> GameSnapshot {
        let mut entities = Vec::new();

        // Include all players in snapshot
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
            let ship = self.world.get::<&Ship>(entity).map(|s| (*s).clone()).ok();

            entities.push(EntitySnapshot {
                entity_id: entity.id() as u64, // Convert hecs::Entity to u64
                entity_type: EntityType::Player(player.clone()),
                transform: transform.clone(),
                velocity,
                health,
                ship, // Include ship configuration for players
            });
        }

        // Include all projectiles in snapshot
        for (entity, (transform, projectile)) in
            self.world.query::<(&Transform, &Projectile)>().iter()
        {
            let velocity = self
                .world
                .get::<&Velocity>(entity)
                .map(|v| (*v).clone())
                .unwrap_or_else(|_| Velocity::default());
            let health = Health::default(); // Projectiles don't have health but we need it for the snapshot

            entities.push(EntitySnapshot {
                entity_id: entity.id() as u64,
                entity_type: EntityType::Projectile(projectile.clone()),
                transform: transform.clone(),
                velocity,
                health,
                ship: None, // Projectiles don't have ship configurations
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
    pub ship: Option<Ship>, // Ship configuration data for players
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EntityType {
    Player(Player),
    Projectile(Projectile),
    Enemy, // Will be expanded in Phase 4
}
