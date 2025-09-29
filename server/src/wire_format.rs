//! Wire format for network messages
//!
//! This module handles serialization and deserialization of game messages
//! using serde for JSON encoding (will be upgraded to binary in future iterations).

#![allow(dead_code)] // Allow unused code during Phase 2 infrastructure development

use crate::components::InputData;
use crate::simulation::GameSnapshot;
use serde::{Deserialize, Serialize};

/// Message wrapper for all network communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryMessage {
    pub message_type: MessageType,
    pub sequence: u32,
    pub timestamp: u64,
}

/// Message type enumeration for wire protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    /// Client input data
    Input(InputData),

    /// Game state snapshot
    Snapshot(GameSnapshot),

    /// Ping/Pong for latency measurement
    Ping {
        timestamp: u64,
    },
    Pong {
        timestamp: u64,
    },

    /// Connection management
    Join {
        room_code: String,
        player_name: String,
    },
    Leave,

    /// Acknowledgments
    Ack {
        sequence: u32,
    },

    /// Error messages
    Error {
        message: String,
    },

    /// Debug rendering data
    DebugRender(DebugRenderData),
}

/// Debug rendering data for physics visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugRenderData {
    pub sequence: u32,
    pub rigid_bodies: Vec<DebugRigidBody>,
    pub colliders: Vec<DebugCollider>,
    pub forces: Vec<DebugForce>,
    pub velocities: Vec<DebugVelocity>,
    pub joints: Vec<DebugJoint>,
}

/// Debug rigid body visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugRigidBody {
    pub handle: u32, // Simplified handle for network
    pub position: [f32; 2],
    pub rotation: f32,
    pub body_type: DebugBodyType,
    pub mass: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
}

/// Debug collider visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugCollider {
    pub handle: u32,
    pub parent_body: u32,
    pub shape: DebugShape,
    pub position: [f32; 2],
    pub rotation: f32,
}

/// Debug force vector visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugForce {
    pub body_handle: u32,
    pub force: [f32; 2],
    pub torque: f32,
    pub application_point: [f32; 2],
}

/// Debug velocity vector visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugVelocity {
    pub body_handle: u32,
    pub linear_velocity: [f32; 2],
    pub angular_velocity: f32,
}

/// Debug joint visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugJoint {
    pub handle: u32,
    pub body1: u32,
    pub body2: u32,
    pub anchor1: [f32; 2],
    pub anchor2: [f32; 2],
    pub joint_type: DebugJointType,
}

/// Simplified body type for debug rendering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DebugBodyType {
    Dynamic,
    Kinematic,
    Static,
}

/// Simplified shape for debug rendering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DebugShape {
    Ball { radius: f32 },
    Cuboid { half_extents: [f32; 2] },
    Triangle { vertices: [[f32; 2]; 3] },
    Polygon { vertices: Vec<[f32; 2]> },
}

/// Simplified joint type for debug rendering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DebugJointType {
    Fixed,
    Revolute,
    Prismatic,
    Distance,
    Spring,
}

impl BinaryMessage {
    pub fn new(message_type: MessageType, sequence: u32) -> Self {
        Self {
            message_type,
            sequence,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        }
    }

    /// Serialize message to JSON bytes (placeholder for binary format)
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(self).map_err(|e| format!("Serialization error: {}", e))
    }

    /// Deserialize message from JSON bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes).map_err(|e| format!("Deserialization error: {}", e))
    }
}

/// Protocol version for compatibility checking
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum message size in bytes (1MB)
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// Message priority levels for bandwidth management
#[derive(Debug, Clone, Copy)]
pub enum MessagePriority {
    /// Critical messages (connection, errors)
    Critical,
    /// High priority (input, acks)
    High,
    /// Normal priority (snapshots)
    Normal,
    /// Low priority (optional data)
    Low,
}

impl MessageType {
    pub fn priority(&self) -> MessagePriority {
        match self {
            MessageType::Join { .. } | MessageType::Leave | MessageType::Error { .. } => {
                MessagePriority::Critical
            }
            MessageType::Input(_) | MessageType::Ack { .. } => MessagePriority::High,
            MessageType::Snapshot(_) => MessagePriority::Normal,
            MessageType::Ping { .. } | MessageType::Pong { .. } => MessagePriority::Low,
            MessageType::DebugRender(_) => MessagePriority::Low, // Debug data is low priority
        }
    }
}
