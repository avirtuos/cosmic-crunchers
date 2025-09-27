//! ECS Components for Cosmic Crunchers
//!
//! This module defines all the components used in the Entity Component System.
//! Components are pure data structures that get attached to entities.

#![allow(dead_code)] // Allow unused code during Phase 2 infrastructure development

use rapier2d::prelude::*;
use serde::{Deserialize as SerdeDeserialize, Serialize as SerdeSerialize};
use std::collections::VecDeque;
use uuid::Uuid;

/// Position and orientation in 2D space
#[derive(Debug, Clone, SerdeSerialize, SerdeDeserialize)]
pub struct Transform {
    pub position: [f32; 2], // Use array instead of rapier Vector for serialization
    pub rotation: f32,      // radians
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0],
            rotation: 0.0,
        }
    }
}

impl Transform {
    pub fn from_vector(position: Vector<f32>, rotation: f32) -> Self {
        Self {
            position: [position.x, position.y],
            rotation,
        }
    }

    pub fn to_vector(&self) -> Vector<f32> {
        Vector::new(self.position[0], self.position[1])
    }
}

/// Linear and angular velocity
#[derive(Debug, Clone, SerdeSerialize, SerdeDeserialize)]
pub struct Velocity {
    pub linear: [f32; 2],
    pub angular: f32, // radians per second
}

impl Default for Velocity {
    fn default() -> Self {
        Self {
            linear: [0.0, 0.0],
            angular: 0.0,
        }
    }
}

impl Velocity {
    pub fn from_vector(linear: Vector<f32>, angular: f32) -> Self {
        Self {
            linear: [linear.x, linear.y],
            angular,
        }
    }

    pub fn to_vector(&self) -> Vector<f32> {
        Vector::new(self.linear[0], self.linear[1])
    }
}

/// Health and damage tracking
#[derive(Debug, Clone, SerdeSerialize, SerdeDeserialize)]
pub struct Health {
    pub current: f32,
    pub max: f32,
    pub armor: f32,
    pub shield: f32,
    pub shield_max: f32,
    pub shield_recharge_rate: f32,
    pub shield_recharge_delay: f32,
    pub last_damage_time: f64,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            current: 100.0,
            max: 100.0,
            armor: 0.0,
            shield: 50.0,
            shield_max: 50.0,
            shield_recharge_rate: 10.0, // per second
            shield_recharge_delay: 3.0, // seconds
            last_damage_time: 0.0,
        }
    }
}

/// Player-specific data
#[derive(Debug, Clone, SerdeSerialize, SerdeDeserialize)]
pub struct Player {
    pub id: Uuid,
    pub name: String,
    pub score: u32,
    pub kills: u32,
    pub deaths: u32,
    pub credits: u32,
}

/// Buffered input data with timestamps
#[derive(Debug, Clone, SerdeSerialize, SerdeDeserialize)]
pub struct InputData {
    pub sequence: u32,
    pub timestamp: u64,
    pub thrust: f32, // 0.0 to 1.0
    pub turn: f32,   // -1.0 to 1.0 (left/right)
    pub primary_fire: bool,
    pub secondary_fire: bool,
}

impl Default for InputData {
    fn default() -> Self {
        Self {
            sequence: 0,
            timestamp: 0,
            thrust: 0.0,
            turn: 0.0,
            primary_fire: false,
            secondary_fire: false,
        }
    }
}

/// Input buffer for processing delayed inputs
#[derive(Debug, Clone)]
pub struct InputBuffer {
    pub buffer: VecDeque<InputData>,
    pub last_processed_sequence: u32,
    pub max_buffer_size: usize,
}

impl Default for InputBuffer {
    fn default() -> Self {
        Self {
            buffer: VecDeque::new(),
            last_processed_sequence: 0,
            max_buffer_size: 60, // ~2 seconds at 30 TPS
        }
    }
}

impl InputBuffer {
    pub fn add_input(&mut self, input: InputData) {
        // Insert in sequence order
        let mut insert_pos = self.buffer.len();
        for (i, existing) in self.buffer.iter().enumerate().rev() {
            if existing.sequence < input.sequence {
                break;
            }
            insert_pos = i;
        }

        self.buffer.insert(insert_pos, input);

        // Trim buffer if too large
        while self.buffer.len() > self.max_buffer_size {
            self.buffer.pop_front();
        }
    }

    pub fn get_next_input(&mut self) -> Option<InputData> {
        if let Some(front) = self.buffer.front()
            && front.sequence == self.last_processed_sequence + 1
        {
            self.last_processed_sequence = front.sequence;
            return self.buffer.pop_front();
        }
        None
    }

    pub fn clear_old_inputs(&mut self, min_sequence: u32) {
        while let Some(front) = self.buffer.front() {
            if front.sequence < min_sequence {
                self.buffer.pop_front();
            } else {
                break;
            }
        }
    }
}

/// Physics body reference
#[derive(Debug, Clone)]
pub struct RigidBody {
    pub handle: RigidBodyHandle,
}

/// Physics collider reference
#[derive(Debug, Clone)]
pub struct Collider {
    pub handle: ColliderHandle,
}

/// Ship-specific properties
#[derive(Debug, Clone, SerdeSerialize, SerdeDeserialize)]
pub struct Ship {
    pub thrust_power: f32,
    pub turn_rate: f32,
    pub max_speed: f32,
    pub mass: f32,
    pub size: f32, // radius for collision
}

impl Default for Ship {
    fn default() -> Self {
        Self {
            thrust_power: 500.0,
            turn_rate: 3.0, // radians per second
            max_speed: 200.0,
            mass: 1.0,
            size: 8.0,
        }
    }
}

/// Projectile-specific properties
#[derive(Debug, Clone, SerdeSerialize, SerdeDeserialize)]
pub struct Projectile {
    pub damage: f32,
    pub lifetime: f32,
    pub speed: f32,
    pub owner_id: Uuid,
}

/// Lifetime tracking for temporary entities
#[derive(Debug, Clone)]
pub struct Lifetime {
    pub remaining: f32, // seconds
}

/// Enemy AI component
#[derive(Debug, Clone)]
pub struct Enemy {
    pub ai_type: EnemyType,
    pub target: Option<hecs::Entity>,
    pub state: EnemyState,
    pub last_action_time: f64,
}

#[derive(Debug, Clone)]
pub enum EnemyType {
    Chaser { speed: f32 },
    Shooter { range: f32, fire_rate: f32 },
}

#[derive(Debug, Clone)]
pub enum EnemyState {
    Idle,
    Seeking,
    Attacking,
    Fleeing,
}

/// Weapon system component
#[derive(Debug, Clone)]
pub struct Weapon {
    pub weapon_type: WeaponType,
    pub last_fire_time: f64,
    pub ammo: Option<u32>, // None for unlimited
    pub cooldown: f32,
}

#[derive(Debug, Clone)]
pub enum WeaponType {
    RapidFire {
        rate: f32,
        damage: f32,
        speed: f32,
    },
    Beam {
        damage_per_second: f32,
        range: f32,
    },
    Spread {
        count: u8,
        spread_angle: f32,
        damage: f32,
        speed: f32,
    },
    Homing {
        damage: f32,
        speed: f32,
        turn_rate: f32,
    },
    AreaNuke {
        damage: f32,
        radius: f32,
    },
}
