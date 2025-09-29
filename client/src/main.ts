import './style.css';
import Phaser from 'phaser';
import RAPIER from '@dimforge/rapier2d-compat';

// Game configuration
const GAME_WIDTH = 1920;
const GAME_HEIGHT = 1080;
const INPUT_SAMPLE_RATE = 15; // TPS to match server (now 15 Hz)
const INTERPOLATION_DELAY = 120; // ms

// Debug rendering data structures (matching server)
interface DebugRenderData {
  sequence: number;
  rigid_bodies: DebugRigidBody[];
  colliders: DebugCollider[];
  forces: DebugForce[];
  velocities: DebugVelocity[];
  joints: DebugJoint[];
}

interface DebugRigidBody {
  handle: number;
  position: [number, number];
  rotation: number;
  body_type: 'Dynamic' | 'Kinematic' | 'Static';
  mass: number;
  linear_damping: number;
  angular_damping: number;
}

interface DebugCollider {
  handle: number;
  parent_body: number;
  shape: DebugShape;
  position: [number, number];
  rotation: number;
}

interface DebugForce {
  body_handle: number;
  force: [number, number];
  torque: number;
  application_point: [number, number];
}

interface DebugVelocity {
  body_handle: number;
  linear_velocity: [number, number];
  angular_velocity: number;
}

interface DebugJoint {
  handle: number;
  body1: number;
  body2: number;
  anchor1: [number, number];
  anchor2: [number, number];
  joint_type: 'Fixed' | 'Revolute' | 'Prismatic' | 'Distance' | 'Spring';
}

type DebugShape =
  | { Ball: { radius: number } }
  | { Cuboid: { half_extents: [number, number] } }
  | { Triangle: { vertices: [[number, number], [number, number], [number, number]] } }
  | { Polygon: { vertices: [number, number][] } };

interface InputData {
  sequence: number;
  timestamp: number;
  thrust: number;
  turn: number;
  primary_fire: boolean;
  secondary_fire: boolean;
}

interface GameSnapshot {
  sequence: number;
  tick: number;
  timestamp: number;
  entities: EntitySnapshot[];
}

interface EntitySnapshot {
  entity_id: number;
  entity_type: any;
  transform: {
    position: [number, number];
    rotation: number;
  };
  velocity: {
    linear: [number, number];
    angular: number;
  };
  health: any;
  ship?: {
    thrust_power: number;
    turn_rate: number;
    max_speed: number;
    mass: number;
    size: number;
  };
}

// Entity interpolation data
interface InterpolationState {
  fromSnapshot: EntitySnapshot;
  toSnapshot: EntitySnapshot;
  startTime: number;
  duration: number;
}

// Input prediction state
interface PredictedState {
  position: [number, number];
  rotation: number;
  velocity: [number, number];
  angularVelocity: number;
}

class GameScene extends Phaser.Scene {
  private client: CosmicCrunchersClient | null = null;
  private localShip: Phaser.GameObjects.Graphics | null = null;
  private remoteShips: Map<number, Phaser.GameObjects.Graphics> = new Map();
  private projectiles: Map<number, Phaser.GameObjects.Graphics> = new Map();
  private interpolationStates: Map<number, InterpolationState> = new Map();

  // Input system
  private keys: any = {};
  private inputSequence: number = 0;
  private inputBuffer: InputData[] = [];
  private lastSentInput: InputData | null = null; // Track last sent input to prevent duplicates

  // Fixed timestep physics (15 Hz to match server)
  private readonly PHYSICS_TIMESTEP = 1000 / 15; // 66.667ms
  private physicsAccumulator = 0;
  private lastPhysicsTime = 0;

  // Heartbeat system for held inputs
  private lastInputSentTime: number = 0;
  private inputHeartbeatInterval: number = this.PHYSICS_TIMESTEP;

  // Rapier physics world (identical to server)
  private physicsWorld: RAPIER.World | null = null;
  private localRigidBody: RAPIER.RigidBody | null = null;

  // Prediction
  private localEntityId: number | null = null;
  private predictedState: PredictedState | null = null;

  // Simple correction tracking
  private lastEmergencyCorrection: number = 0;

  // Debug rendering
  private debugRenderEnabled: boolean = false;
  private debugGraphics: Phaser.GameObjects.Graphics | null = null;
  private debugRenderData: DebugRenderData | null = null;

  // Debug
  private debugText: Phaser.GameObjects.Text | null = null;
  private debugInfo = {
    rtt: 0,
    snapshotAge: 0,
    corrections: 0,
    entityCount: 0,
    positionDrift: 0,
    velocityDiff: 0,
    maxDrift: 0,
    driftRate: 0,
    lastDrift: 0,
    driftHistory: [] as number[],
    positionCorrectionForce: 0,
    velocityCorrectionForce: 0,
    rotationCorrectionForce: 0,
    // Server authoritative state
    serverPosition: [0, 0] as [number, number],
    serverVelocity: [0, 0] as [number, number],
    serverRotation: 0,
    // Ship configuration from server
    shipConfig: {
      mass: 1.0,
      thrust_power: 2000.0,
      turn_rate: 5.0,
      max_speed: 100.0,
      size: 8.0
    }
  };

  constructor() {
    super({ key: 'GameScene' });
  }

  preload() {
    // We'll use simple graphics for now, no sprites needed
  }

  async create() {
    // Initialize Rapier physics world (identical to server)
    await this.initializeRapierPhysics();

    // Set world bounds to match server
    this.physics.world.setBounds(-GAME_WIDTH / 2, -GAME_HEIGHT / 2, GAME_WIDTH, GAME_HEIGHT);

    // Create background
    this.add.rectangle(0, 0, GAME_WIDTH, GAME_HEIGHT, 0x000011);

    // Add some stars for atmosphere
    for (let i = 0; i < 200; i++) {
      const x = Phaser.Math.Between(-GAME_WIDTH / 2, GAME_WIDTH / 2);
      const y = Phaser.Math.Between(-GAME_HEIGHT / 2, GAME_HEIGHT / 2);
      const size = Phaser.Math.FloatBetween(0.5, 2);
      this.add.circle(x, y, size, 0xffffff);
    }

    // Setup camera
    this.cameras.main.setZoom(0.8);
    this.cameras.main.centerOn(0, 0);

    // Setup input
    this.setupInput();

    // Create debug overlay
    this.createDebugOverlay();

    // Start input sampling
    this.time.addEvent({
      delay: 1000 / INPUT_SAMPLE_RATE,
      callback: this.sampleInput,
      callbackScope: this,
      loop: true
    });

    // Initialize physics timestep
    this.lastPhysicsTime = performance.now();

    console.log('Game scene created with Rapier physics');
  }

  async initializeRapierPhysics() {
    // Initialize Rapier (must be awaited)
    await RAPIER.init();

    // Create physics world identical to server
    const gravity = { x: 0.0, y: 0.0 }; // No gravity in space (match server)
    this.physicsWorld = new RAPIER.World(gravity);

    console.log('Rapier physics world initialized');
  }

  setupInput() {
    this.keys = {
      W: this.input.keyboard!.addKey('W'),
      A: this.input.keyboard!.addKey('A'),
      S: this.input.keyboard!.addKey('S'),
      D: this.input.keyboard!.addKey('D'),
      UP: this.input.keyboard!.addKey('UP'),
      LEFT: this.input.keyboard!.addKey('LEFT'),
      DOWN: this.input.keyboard!.addKey('DOWN'),
      RIGHT: this.input.keyboard!.addKey('RIGHT'),
      SPACE: this.input.keyboard!.addKey('SPACE'),
      SHIFT: this.input.keyboard!.addKey('SHIFT'),
      X: this.input.keyboard!.addKey('X')
    };

    // Add debug toggle listener
    this.keys.X.on('down', () => {
      this.toggleDebugRender();
    });
  }

  createDebugOverlay() {
    this.debugText = this.add.text(10, 10, '', {
      font: '14px monospace',
      color: '#ffffff',
      backgroundColor: 'rgba(0,0,0,0.7)',
      padding: { x: 10, y: 10 }
    });
    this.debugText.setScrollFactor(0);
    this.debugText.setDepth(1000);
  }

  sampleInput() {
    if (!this.client?.isConnectedToRoom()) return;

    // Calculate input values
    let thrust = 0;
    let turn = 0;

    // Thrust input (W/S or UP/DOWN)
    if (this.keys.W.isDown || this.keys.UP.isDown) thrust = 1;
    if (this.keys.S.isDown || this.keys.DOWN.isDown) thrust = -0.5; // Reverse thrust

    // Turn input (A/D or LEFT/RIGHT)
    if (this.keys.A.isDown || this.keys.LEFT.isDown) turn = -1;
    if (this.keys.D.isDown || this.keys.RIGHT.isDown) turn = 1;

    // Create input data
    const input: InputData = {
      sequence: ++this.inputSequence,
      timestamp: Date.now(),
      thrust,
      turn,
      primary_fire: this.keys.SPACE.isDown,
      secondary_fire: this.keys.SHIFT.isDown
    };

    // Always store input for prediction (even if not sending to server)
    this.inputBuffer.push(input);

    // Keep buffer manageable (2 seconds worth)
    if (this.inputBuffer.length > INPUT_SAMPLE_RATE * 2) {
      this.inputBuffer.shift();
    }

    // HYBRID HEARTBEAT SYSTEM
    const now = Date.now();
    const timeSinceLastSent = now - this.lastInputSentTime;

    // Check if input has actually changed from the last sent input
    const inputChanged = !this.lastSentInput ||
      this.lastSentInput.thrust !== input.thrust ||
      this.lastSentInput.turn !== input.turn ||
      this.lastSentInput.primary_fire !== input.primary_fire ||
      this.lastSentInput.secondary_fire !== input.secondary_fire;

    // Check if any keys are currently held down
    const keysHeld = thrust !== 0 || turn !== 0 || input.primary_fire || input.secondary_fire;

    // Send input if:
    // 1. Input has changed (immediate response)
    // 2. OR keys are held AND heartbeat interval has elapsed (persistent input)
    const shouldSend = inputChanged ||
      (keysHeld && timeSinceLastSent >= this.inputHeartbeatInterval);

    if (shouldSend) {
      this.client?.sendInput(input);
      this.lastSentInput = { ...input }; // Store copy of sent input
      this.lastInputSentTime = now; // Update last sent time

      if (inputChanged) {
        console.log('Input changed - sent to server:', input.sequence, 'thrust:', input.thrust, 'turn:', input.turn);
      } else {
        console.log('Input heartbeat - sent to server:', input.sequence, 'thrust:', input.thrust, 'turn:', input.turn, `(${timeSinceLastSent}ms since last)`);
      }
    }

    // Always apply local prediction regardless of network sending
    this.applyLocalPrediction(input);
  }

  applyLocalPrediction(input: InputData) {
    if (!this.localShip || !this.predictedState || !this.localRigidBody) return;

    // Apply forces through Rapier physics (matching server exactly)

    //We reset the forces every tick of the simulation because we don't presently
    //support cumulative forces.
    this.localRigidBody.resetForces(true);
    this.localRigidBody.resetTorques(true);

    // Apply thrust force (using server-provided config)
    if (input.thrust !== 0) {
      const thrustPower = this.debugInfo.shipConfig.thrust_power;
      const rotation = this.localRigidBody.rotation();
      const thrustDirection = {
        x: Math.cos(rotation),
        y: Math.sin(rotation)
      };

      const thrustForce = {
        x: thrustDirection.x * thrustPower * input.thrust,
        y: thrustDirection.y * thrustPower * input.thrust
      };

      this.localRigidBody.addForce(thrustForce, true);
    }

    // Apply turning torque (using server-provided config)
    if (input.turn !== 0) {
      const turnRate = this.debugInfo.shipConfig.turn_rate;
      const mass = this.debugInfo.shipConfig.mass;
      const torque = -input.turn * turnRate * mass * 100.0; // Match server calculation
      this.localRigidBody.addTorque(torque, true);
    }

    // Apply boundaries using Rapier body position
    const translation = this.localRigidBody.translation();
    const halfWidth = GAME_WIDTH / 2;
    const halfHeight = GAME_HEIGHT / 2;

    let needsBoundaryCorrection = false;
    let newPos = { x: translation.x, y: translation.y };

    if (translation.x < -halfWidth) {
      newPos.x = -halfWidth;
      needsBoundaryCorrection = true;
    } else if (translation.x > halfWidth) {
      newPos.x = halfWidth;
      needsBoundaryCorrection = true;
    }

    if (translation.y < -halfHeight) {
      newPos.y = -halfHeight;
      needsBoundaryCorrection = true;
    } else if (translation.y > halfHeight) {
      newPos.y = halfHeight;
      needsBoundaryCorrection = true;
    }

    if (needsBoundaryCorrection) {
      this.localRigidBody.setTranslation(newPos, true);
      // Reduce velocity when hitting boundaries (match server)
      const velocity = this.localRigidBody.linvel();
      this.localRigidBody.setLinvel({ x: velocity.x * 0.5, y: velocity.y * 0.5 }, true);
    }

    // The fixed timestep physics loop will handle the actual integration
    // and update the predictedState through syncRapierToPredictedState()
  }

  updateLocalShipVisual() {
    if (!this.localShip || !this.predictedState) return;

    this.localShip.setPosition(
      this.predictedState.position[0],
      this.predictedState.position[1]
    );
    this.localShip.setRotation(this.predictedState.rotation);

    // Update camera to follow local ship
    this.cameras.main.centerOn(
      this.predictedState.position[0],
      this.predictedState.position[1]
    );
  }

  handleSnapshot(snapshot: GameSnapshot) {
    const now = Date.now();
    this.debugInfo.snapshotAge = now - snapshot.timestamp;
    this.debugInfo.entityCount = snapshot.entities.length;

    // Track existing entities to clean up removed ones
    const seenEntities = new Set<number>();

    for (const entity of snapshot.entities) {
      seenEntities.add(entity.entity_id);

      if (entity.entity_id === this.localEntityId) {
        // Handle local player reconciliation
        this.handleLocalPlayerReconciliation(entity);
      } else if (entity.entity_type.Player) {
        // Handle remote player interpolation
        this.handleRemotePlayerUpdate(entity, now);
      } else if (entity.entity_type.Projectile) {
        // Handle projectile updates
        this.handleProjectileUpdate(entity);
      }
    }

    // Clean up projectiles that are no longer in the snapshot
    for (const [entityId, projectile] of this.projectiles.entries()) {
      if (!seenEntities.has(entityId)) {
        projectile.destroy();
        this.projectiles.delete(entityId);
      }
    }
  }

  handleProjectileUpdate(entity: EntitySnapshot) {
    const entityId = entity.entity_id;

    if (!this.projectiles.has(entityId)) {
      // Create new projectile visual
      this.createProjectile(entityId);
    }

    // Update projectile position (no interpolation needed for fast projectiles)
    const projectile = this.projectiles.get(entityId);
    if (projectile) {
      projectile.setPosition(
        entity.transform.position[0],
        entity.transform.position[1]
      );
      projectile.setRotation(entity.transform.rotation);
    }
  }

  createProjectile(entityId: number) {
    const projectile = this.add.graphics();
    this.drawProjectile(projectile);
    this.projectiles.set(entityId, projectile);
  }

  drawProjectile(graphics: Phaser.GameObjects.Graphics) {
    graphics.clear();
    graphics.fillStyle(0xffff00, 1.0); // Bright yellow
    graphics.lineStyle(1, 0xffffff, 0.8); // White outline

    // Draw small circular projectile
    graphics.fillCircle(0, 0, 3);
    graphics.strokeCircle(0, 0, 3);
  }

  handleLocalPlayerReconciliation(serverEntity: EntitySnapshot) {
    if (!this.predictedState || !this.localRigidBody) {
      // Initialize prediction from server state
      this.predictedState = {
        position: [...serverEntity.transform.position],
        rotation: serverEntity.transform.rotation,
        velocity: [...serverEntity.velocity.linear],
        angularVelocity: serverEntity.velocity.angular
      };
      this.createLocalShip();
      return;
    }

    // Get current rigid body state for comparison
    const rigidBodyPos = this.localRigidBody.translation();
    const rigidBodyVel = this.localRigidBody.linvel();
    const rigidBodyRot = this.localRigidBody.rotation();
    const rigidBodyAngVel = this.localRigidBody.angvel();
    
    const currentPos = [rigidBodyPos.x, rigidBodyPos.y];
    const currentVel = [rigidBodyVel.x, rigidBodyVel.y];

    // Calculate position and velocity differences for debug tracking
    const serverPos = serverEntity.transform.position;
    const serverVel = serverEntity.velocity.linear;
    const serverRot = serverEntity.transform.rotation;
    const serverAngVel = serverEntity.velocity.angular;

    const distance = Math.sqrt(
      (serverPos[0] - currentPos[0]) ** 2 + (serverPos[1] - currentPos[1]) ** 2
    );

    const velocityDiff = Math.sqrt(
      (serverVel[0] - currentVel[0]) ** 2 + (serverVel[1] - currentVel[1]) ** 2
    );

    // Calculate rotation difference (handle wrap-around)
    let rotationDiff = Math.abs(serverRot - rigidBodyRot);
    if (rotationDiff > Math.PI) {
      rotationDiff = 2 * Math.PI - rotationDiff;
    }

    // Update debug tracking
    this.debugInfo.positionDrift = distance;
    this.debugInfo.velocityDiff = velocityDiff;
    this.debugInfo.maxDrift = Math.max(this.debugInfo.maxDrift, distance);
    this.debugInfo.serverPosition = [...serverPos];
    this.debugInfo.serverVelocity = [...serverVel];
    this.debugInfo.serverRotation = serverRot;

    // Extract ship configuration from server snapshot
    if (serverEntity.ship) {
      this.debugInfo.shipConfig = {
        mass: serverEntity.ship.mass,
        thrust_power: serverEntity.ship.thrust_power,
        turn_rate: serverEntity.ship.turn_rate,
        max_speed: serverEntity.ship.max_speed,
        size: serverEntity.ship.size
      };
    }

    // Calculate drift rate for debug display
    if (this.debugInfo.lastDrift > 0) {
      const timeDelta = 67; // 15Hz ≈ 67ms between snapshots
      this.debugInfo.driftRate = (distance - this.debugInfo.lastDrift) / (timeDelta / 1000);
    }
    this.debugInfo.lastDrift = distance;

    // Add to drift history for trends (keep last 10 values)
    this.debugInfo.driftHistory.push(distance);
    if (this.debugInfo.driftHistory.length > 10) {
      this.debugInfo.driftHistory.shift();
    }

    // THREE-TIER CORRECTION SYSTEM
    const now = Date.now();

    // EMERGENCY CORRECTIONS: Large drifts that indicate serious desync
    if (distance > 50) {
      // Prevent spam corrections (minimum 100ms between emergency corrections)
      if (now - this.lastEmergencyCorrection > 100) {
        this.debugInfo.corrections++;
        this.lastEmergencyCorrection = now;

        console.log(`🚨 EMERGENCY CORRECTION: ${distance.toFixed(1)}px drift - hard reset to server state`);

        // Hard reset to server authoritative state
        this.localRigidBody.setTranslation({ x: serverPos[0], y: serverPos[1] }, true);
        this.localRigidBody.setLinvel({ x: serverVel[0], y: serverVel[1] }, true);
        this.localRigidBody.setRotation(serverRot, true);
        this.localRigidBody.setAngvel(serverAngVel, true);

        // Sync predicted state
        this.syncRapierToPredictedState();

        console.log(`✅ Emergency correction applied - physics synchronized`);
      }
    } else {
      // CONTINUOUS CONVERGENCE SYSTEM: Apply 1% corrections when differences exceed 5%
      
      // Define 5% thresholds for each property
      const positionThreshold = this.debugInfo.shipConfig.size * 0.05; // 5% of ship size (~0.4px)
      const velocityThreshold = this.debugInfo.shipConfig.max_speed * 0.05; // 5% of max speed (~5px/s)
      const rotationThreshold = Math.PI * 2 * 0.05; // 5% of full rotation (~0.314 radians)
      const angularVelocityThreshold = this.debugInfo.shipConfig.turn_rate * 0.05; // 5% of turn rate

      let appliedCorrections = 0;

      // POSITION CONVERGENCE
      if (distance > positionThreshold) {
        const correctionFactor = 0.01; // 1% correction
        const correctionX = (serverPos[0] - currentPos[0]) * correctionFactor;
        const correctionY = (serverPos[1] - currentPos[1]) * correctionFactor;
        
        this.localRigidBody.setTranslation({
          x: currentPos[0] + correctionX,
          y: currentPos[1] + correctionY
        }, true);
        
        this.debugInfo.positionCorrectionForce = Math.sqrt(correctionX * correctionX + correctionY * correctionY);
        appliedCorrections++;
      } else {
        this.debugInfo.positionCorrectionForce = 0;
      }

      // VELOCITY CONVERGENCE
      if (velocityDiff > velocityThreshold) {
        const correctionFactor = 0.01; // 1% correction
        const correctionVelX = (serverVel[0] - currentVel[0]) * correctionFactor;
        const correctionVelY = (serverVel[1] - currentVel[1]) * correctionFactor;
        
        this.localRigidBody.setLinvel({
          x: currentVel[0] + correctionVelX,
          y: currentVel[1] + correctionVelY
        }, true);
        
        this.debugInfo.velocityCorrectionForce = Math.sqrt(correctionVelX * correctionVelX + correctionVelY * correctionVelY);
        appliedCorrections++;
      } else {
        this.debugInfo.velocityCorrectionForce = 0;
      }

      // ROTATION CONVERGENCE
      if (rotationDiff > rotationThreshold) {
        const correctionFactor = 0.01; // 1% correction
        
        // Calculate shortest rotation direction
        let rotationCorrection = serverRot - rigidBodyRot;
        if (rotationCorrection > Math.PI) {
          rotationCorrection -= 2 * Math.PI;
        } else if (rotationCorrection < -Math.PI) {
          rotationCorrection += 2 * Math.PI;
        }
        
        const correctionRot = rotationCorrection * correctionFactor;
        this.localRigidBody.setRotation(rigidBodyRot + correctionRot, true);
        
        this.debugInfo.rotationCorrectionForce = Math.abs(correctionRot);
        appliedCorrections++;
      } else {
        this.debugInfo.rotationCorrectionForce = 0;
      }

      // ANGULAR VELOCITY CONVERGENCE
      const angularVelocityDiff = Math.abs(serverAngVel - rigidBodyAngVel);
      if (angularVelocityDiff > angularVelocityThreshold) {
        const correctionFactor = 0.01; // 1% correction
        const correctionAngVel = (serverAngVel - rigidBodyAngVel) * correctionFactor;
        
        this.localRigidBody.setAngvel(rigidBodyAngVel + correctionAngVel, true);
        appliedCorrections++;
      }

      // Debug logging for convergence activity
      if (appliedCorrections > 0) {
        console.log(`🔧 Convergence: Applied ${appliedCorrections} corrections - pos:${distance.toFixed(1)}px vel:${velocityDiff.toFixed(1)}px/s rot:${rotationDiff.toFixed(3)}rad`);
      }
    }

    this.updateLocalShipVisual();
  }

  handleRemotePlayerUpdate(entity: EntitySnapshot, currentTime: number) {
    const entityId = entity.entity_id;

    if (!this.remoteShips.has(entityId)) {
      // Create new remote ship
      this.createRemoteShip(entityId);
    }

    // Set up interpolation
    const prevState = this.interpolationStates.get(entityId);
    if (prevState && prevState.toSnapshot) {
      // Update interpolation with new target
      this.interpolationStates.set(entityId, {
        fromSnapshot: prevState.toSnapshot,
        toSnapshot: entity,
        startTime: currentTime - INTERPOLATION_DELAY,
        duration: 1000 / 15 // 15 Hz snapshot rate
      });
    } else {
      // First snapshot for this entity
      this.interpolationStates.set(entityId, {
        fromSnapshot: entity,
        toSnapshot: entity,
        startTime: currentTime - INTERPOLATION_DELAY,
        duration: 0
      });
    }
  }

  update() {
    // Fixed timestep physics update
    this.updateFixedTimestepPhysics();

    // Update remote ship interpolation
    const now = Date.now() - INTERPOLATION_DELAY;

    for (const [entityId, interpolationState] of this.interpolationStates.entries()) {
      const ship = this.remoteShips.get(entityId);
      if (!ship) continue;

      const elapsed = now - interpolationState.startTime;
      const progress = Math.min(elapsed / interpolationState.duration, 1.0);

      if (progress >= 1.0) {
        // Use final position
        const target = interpolationState.toSnapshot;
        ship.setPosition(target.transform.position[0], target.transform.position[1]);
        ship.setRotation(target.transform.rotation);
      } else {
        // Interpolate between positions
        const from = interpolationState.fromSnapshot;
        const to = interpolationState.toSnapshot;

        const lerpedX = from.transform.position[0] + (to.transform.position[0] - from.transform.position[0]) * progress;
        const lerpedY = from.transform.position[1] + (to.transform.position[1] - from.transform.position[1]) * progress;
        const lerpedRotation = from.transform.rotation + (to.transform.rotation - from.transform.rotation) * progress;

        ship.setPosition(lerpedX, lerpedY);
        ship.setRotation(lerpedRotation);
      }
    }

    // Update debug text
    this.updateDebugDisplay();
  }

  updateDebugDisplay() {
    if (!this.debugText) return;

    // Color coding for drift
    const driftColor = this.debugInfo.positionDrift < 20 ? '🟢' :
      this.debugInfo.positionDrift < 50 ? '🟡' : '🔴';

    // Drift trend indicator
    const driftHistory = this.debugInfo.driftHistory;
    let trendIndicator = '→';
    if (driftHistory.length >= 3) {
      const recent = driftHistory.slice(-3);
      const increasing = recent[2] > recent[1] && recent[1] > recent[0];
      const decreasing = recent[2] < recent[1] && recent[1] < recent[0];
      trendIndicator = increasing ? '↗' : decreasing ? '↘' : '→';
    }

    // Client predicted state display
    const clientPos = this.predictedState ?
      `[${this.predictedState.position[0].toFixed(1)}, ${this.predictedState.position[1].toFixed(1)}]` :
      '[0.0, 0.0]';
    const clientVel = this.predictedState ?
      `[${this.predictedState.velocity[0].toFixed(1)}, ${this.predictedState.velocity[1].toFixed(1)}]` :
      '[0.0, 0.0]';
    const clientRot = this.predictedState ?
      this.predictedState.rotation.toFixed(2) :
      '0.00';

    // Server authoritative state display
    const serverPos = `[${this.debugInfo.serverPosition[0].toFixed(1)}, ${this.debugInfo.serverPosition[1].toFixed(1)}]`;
    const serverVel = `[${this.debugInfo.serverVelocity[0].toFixed(1)}, ${this.debugInfo.serverVelocity[1].toFixed(1)}]`;
    const serverRot = this.debugInfo.serverRotation.toFixed(2);

    const debugLines = [
      'COSMIC CRUNCHERS - Phase 3',
      '========================',
      `RTT: ${this.debugInfo.rtt}ms | Snapshot Age: ${this.debugInfo.snapshotAge}ms`,
      `Corrections: ${this.debugInfo.corrections} | Entities: ${this.debugInfo.entityCount} | Input Seq: ${this.inputSequence}`,
      '',
      'SHIP CONFIGURATION:',
      `Mass: ${this.debugInfo.shipConfig.mass.toFixed(1)}kg | Size: ${this.debugInfo.shipConfig.size.toFixed(1)}px`,
      `Thrust Power: ${this.debugInfo.shipConfig.thrust_power.toFixed(0)}N`,
      `Turn Rate: ${this.debugInfo.shipConfig.turn_rate.toFixed(1)} rad/s`,
      `Max Speed: ${this.debugInfo.shipConfig.max_speed.toFixed(1)} px/s`,
      '',
      'CLIENT PREDICTION:',
      `Position: ${clientPos}`,
      `Velocity: ${clientVel} px/s`,
      `Rotation: ${clientRot} rad`,
      '',
      'SERVER AUTHORITATIVE:',
      `Position: ${serverPos}`,
      `Velocity: ${serverVel} px/s`,
      `Rotation: ${serverRot} rad`,
      '',
      'DIFFERENCES:',
      `${driftColor} Position Drift: ${this.debugInfo.positionDrift.toFixed(1)}px ${trendIndicator}`,
      `Velocity Diff: ${this.debugInfo.velocityDiff.toFixed(1)} px/s`,
      `Rotation Diff: ${(this.debugInfo.serverRotation - (this.predictedState?.rotation || 0)).toFixed(3)} rad`,
      `Max Drift: ${this.debugInfo.maxDrift.toFixed(1)}px | Rate: ${this.debugInfo.driftRate >= 0 ? '+' : ''}${this.debugInfo.driftRate.toFixed(1)} px/s`,
      '',
      'CONVERGENCE SYSTEM (1% corrections when >5% difference):',
      `Position: ${this.debugInfo.positionDrift.toFixed(3)}px → ${this.debugInfo.positionCorrectionForce > 0 ? '🔧 ACTIVE' : '⚪ IDLE'} (>${(this.debugInfo.shipConfig.size * 0.05).toFixed(2)}px)`,
      `Velocity: ${this.debugInfo.velocityDiff.toFixed(1)}px/s → ${this.debugInfo.velocityCorrectionForce > 0 ? '🔧 ACTIVE' : '⚪ IDLE'} (>${(this.debugInfo.shipConfig.max_speed * 0.05).toFixed(1)}px/s)`,
      `Rotation: ${this.debugInfo.rotationCorrectionForce.toFixed(4)}rad → ${this.debugInfo.rotationCorrectionForce > 0 ? '🔧 ACTIVE' : '⚪ IDLE'} (>${(Math.PI * 2 * 0.05).toFixed(3)}rad)`,
      `Applied Forces: Pos:${this.debugInfo.positionCorrectionForce.toFixed(3)}px | Vel:${this.debugInfo.velocityCorrectionForce.toFixed(1)}px/s`,
      '',
      'Controls: W/↑-Thrust | A/D←/→-Turn | Space-Fire | Shift-SecFire'
    ];

    this.debugText.setText(debugLines.join('\n'));
  }

  createLocalShip() {
    if (this.localShip) this.localShip.destroy();

    this.localShip = this.add.graphics();
    this.drawShip(this.localShip, 0x00ff00); // Green for local player

    // Create Rapier rigid body (identical to server)
    this.createLocalRigidBody();
  }

  createLocalRigidBody() {
    if (!this.physicsWorld || !this.predictedState) return;

    // Create rigid body descriptor (matching server exactly)
    const rigidBodyDesc = RAPIER.RigidBodyDesc.dynamic()
      .setTranslation(this.predictedState.position[0], this.predictedState.position[1])
      .setRotation(this.predictedState.rotation)
      .setLinvel(this.predictedState.velocity[0], this.predictedState.velocity[1])
      .setAngvel(this.predictedState.angularVelocity)
      .setLinearDamping(0.4) // Realistic damping for smooth gameplay - MATCH SERVER
      .setAngularDamping(1.0); // Realistic damping for smooth gameplay - MATCH SERVER

    this.localRigidBody = this.physicsWorld.createRigidBody(rigidBodyDesc);

    // Create collider (using server-provided config)
    const colliderDesc = RAPIER.ColliderDesc.ball(this.debugInfo.shipConfig.size) // Ship radius from server config
      .setDensity(1.0)
      .setFriction(0.0)
      .setRestitution(0.8);

    this.physicsWorld.createCollider(colliderDesc, this.localRigidBody);

    console.log(`Local Ship Mass: ${this.localRigidBody.mass()}`)

    console.log('Local Rapier rigid body created');
  }

  createRemoteShip(entityId: number) {
    const ship = this.add.graphics();
    this.drawShip(ship, 0xff0000); // Red for remote players
    this.remoteShips.set(entityId, ship);
  }

  drawShip(graphics: Phaser.GameObjects.Graphics, color: number) {
    graphics.clear();
    graphics.lineStyle(2, color);
    graphics.fillStyle(color, 0.6);

    // Draw triangular ship
    graphics.beginPath();
    graphics.moveTo(12, 0);  // Front point
    graphics.lineTo(-8, -6); // Back left
    graphics.lineTo(-4, 0);  // Back center
    graphics.lineTo(-8, 6);  // Back right
    graphics.closePath();
    graphics.fillPath();
    graphics.strokePath();

    // Draw thrust indicator (optional visual feedback)
    graphics.lineStyle(1, color);
    graphics.strokeCircle(0, 0, 8);
  }

  setClient(client: CosmicCrunchersClient) {
    this.client = client;
  }

  setLocalEntityId(entityId: number) {
    this.localEntityId = entityId;
  }

  updateFixedTimestepPhysics() {
    if (!this.physicsWorld || !this.localRigidBody) return;

    const currentTime = performance.now();
    const deltaTime = currentTime - this.lastPhysicsTime;
    this.physicsAccumulator += deltaTime;
    this.lastPhysicsTime = currentTime;

    // Fixed timestep loop with accumulator (15 Hz like server)
    while (this.physicsAccumulator >= this.PHYSICS_TIMESTEP) {
      // Step physics world exactly like server
      this.physicsWorld.timestep = this.PHYSICS_TIMESTEP / 1000; // Convert to seconds
      this.physicsWorld.step();

      // Update predicted state from Rapier body
      this.syncRapierToPredictedState();

      // Decrease accumulator
      this.physicsAccumulator -= this.PHYSICS_TIMESTEP;
    }
  }

  syncRapierToPredictedState() {
    if (!this.localRigidBody || !this.predictedState) return;

    // Sync position and rotation
    const translation = this.localRigidBody.translation();
    const rotation = this.localRigidBody.rotation();

    this.predictedState.position[0] = translation.x;
    this.predictedState.position[1] = translation.y;
    this.predictedState.rotation = rotation;

    // Sync velocities
    const linvel = this.localRigidBody.linvel();
    const angvel = this.localRigidBody.angvel();

    this.predictedState.velocity[0] = linvel.x;
    this.predictedState.velocity[1] = linvel.y;
    this.predictedState.angularVelocity = angvel;
  }

  toggleDebugRender() {
    this.debugRenderEnabled = !this.debugRenderEnabled;

    if (this.debugRenderEnabled) {
      console.log('🔍 Debug rendering enabled - Press X to toggle');
      // Create debug graphics layer
      if (!this.debugGraphics) {
        this.debugGraphics = this.add.graphics();
        this.debugGraphics.setDepth(500); // Above game objects but below UI
      }
      // Request debug data from server
      this.client?.requestDebugData();
    } else {
      console.log('🔍 Debug rendering disabled');
      // Clear and hide debug graphics
      if (this.debugGraphics) {
        this.debugGraphics.clear();
        this.debugGraphics.setVisible(false);
      }
    }
  }

  handleDebugRenderData(debugData: DebugRenderData) {
    if (!this.debugRenderEnabled || !this.debugGraphics) return;

    this.debugRenderData = debugData;
    this.renderDebugData();
  }

  private renderDebugData() {
    if (!this.debugGraphics || !this.debugRenderData) return;

    this.debugGraphics.clear();
    this.debugGraphics.setVisible(true);

    // Render rigid bodies
    for (const body of this.debugRenderData.rigid_bodies) {
      this.renderDebugRigidBody(body);
    }

    // Render colliders
    for (const collider of this.debugRenderData.colliders) {
      this.renderDebugCollider(collider);
    }

    // Render velocity vectors
    for (const velocity of this.debugRenderData.velocities) {
      this.renderDebugVelocity(velocity);
    }

    // Render joints
    for (const joint of this.debugRenderData.joints) {
      this.renderDebugJoint(joint);
    }

    // Render forces (if any)
    for (const force of this.debugRenderData.forces) {
      this.renderDebugForce(force);
    }
  }

  private renderDebugRigidBody(body: DebugRigidBody) {
    // Choose color based on body type
    let color = 0xffffff; // White for static
    switch (body.body_type) {
      case 'Dynamic':
        color = 0x00ff00; // Green for dynamic
        break;
      case 'Kinematic':
        color = 0x0080ff; // Blue for kinematic
        break;
    }

    // Draw center of mass
    this.debugGraphics!.fillStyle(color, 0.8);
    this.debugGraphics!.fillCircle(body.position[0], body.position[1], 3);

    // Draw orientation indicator
    const length = 10;
    const endX = body.position[0] + Math.cos(body.rotation) * length;
    const endY = body.position[1] + Math.sin(body.rotation) * length;

    this.debugGraphics!.lineStyle(2, color, 0.6);
    this.debugGraphics!.lineBetween(
      body.position[0], body.position[1],
      endX, endY
    );
  }

  private renderDebugCollider(collider: DebugCollider) {
    const color = 0xff8800; // Orange for colliders
    this.debugGraphics!.lineStyle(1, color, 0.7);

    // Transform to world position
    const worldX = collider.position[0];
    const worldY = collider.position[1];

    // Render based on shape type
    if ('Ball' in collider.shape) {
      const radius = collider.shape.Ball.radius;
      this.debugGraphics!.strokeCircle(worldX, worldY, radius);
    } else if ('Cuboid' in collider.shape) {
      const halfExtents = collider.shape.Cuboid.half_extents;
      this.debugGraphics!.strokeRect(
        worldX - halfExtents[0],
        worldY - halfExtents[1],
        halfExtents[0] * 2,
        halfExtents[1] * 2
      );
    } else if ('Triangle' in collider.shape) {
      const vertices = collider.shape.Triangle.vertices;
      this.debugGraphics!.beginPath();
      this.debugGraphics!.moveTo(
        worldX + vertices[0][0],
        worldY + vertices[0][1]
      );
      for (let i = 1; i < vertices.length; i++) {
        this.debugGraphics!.lineTo(
          worldX + vertices[i][0],
          worldY + vertices[i][1]
        );
      }
      this.debugGraphics!.closePath();
      this.debugGraphics!.strokePath();
    } else if ('Polygon' in collider.shape) {
      const vertices = collider.shape.Polygon.vertices;
      this.debugGraphics!.beginPath();
      this.debugGraphics!.moveTo(
        worldX + vertices[0][0],
        worldY + vertices[0][1]
      );
      for (let i = 1; i < vertices.length; i++) {
        this.debugGraphics!.lineTo(
          worldX + vertices[i][0],
          worldY + vertices[i][1]
        );
      }
      this.debugGraphics!.closePath();
      this.debugGraphics!.strokePath();
    }
  }

  private renderDebugVelocity(velocity: DebugVelocity) {
    // Find the rigid body position
    const body = this.debugRenderData?.rigid_bodies.find(b => b.handle === velocity.body_handle);
    if (!body) return;

    const color = 0xff00ff; // Magenta for velocity vectors
    const scale = 0.1; // Scale down the velocity vectors for visibility

    // Linear velocity vector
    const velMagnitude = Math.sqrt(velocity.linear_velocity[0] ** 2 + velocity.linear_velocity[1] ** 2);
    if (velMagnitude > 1) { // Only show significant velocities
      const endX = body.position[0] + velocity.linear_velocity[0] * scale;
      const endY = body.position[1] + velocity.linear_velocity[1] * scale;

      this.debugGraphics!.lineStyle(2, color, 0.8);
      this.debugGraphics!.lineBetween(
        body.position[0], body.position[1],
        endX, endY
      );

      // Arrow head
      const angle = Math.atan2(velocity.linear_velocity[1], velocity.linear_velocity[0]);
      const arrowSize = 5;
      this.debugGraphics!.lineBetween(
        endX, endY,
        endX - Math.cos(angle - 0.5) * arrowSize,
        endY - Math.sin(angle - 0.5) * arrowSize
      );
      this.debugGraphics!.lineBetween(
        endX, endY,
        endX - Math.cos(angle + 0.5) * arrowSize,
        endY - Math.sin(angle + 0.5) * arrowSize
      );
    }
  }

  private renderDebugJoint(joint: DebugJoint) {
    const color = 0x8080ff; // Light blue for joints
    this.debugGraphics!.lineStyle(2, color, 0.6);

    // Draw line between anchor points
    this.debugGraphics!.lineBetween(
      joint.anchor1[0], joint.anchor1[1],
      joint.anchor2[0], joint.anchor2[1]
    );

    // Draw anchor points
    this.debugGraphics!.fillStyle(color, 0.8);
    this.debugGraphics!.fillCircle(joint.anchor1[0], joint.anchor1[1], 2);
    this.debugGraphics!.fillCircle(joint.anchor2[0], joint.anchor2[1], 2);
  }

  private renderDebugForce(force: DebugForce) {
    const color = 0xffff00; // Yellow for forces
    const scale = 0.01; // Scale down forces for visibility

    const magnitude = Math.sqrt(force.force[0] ** 2 + force.force[1] ** 2);
    if (magnitude > 10) { // Only show significant forces
      const endX = force.application_point[0] + force.force[0] * scale;
      const endY = force.application_point[1] + force.force[1] * scale;

      this.debugGraphics!.lineStyle(3, color, 0.9);
      this.debugGraphics!.lineBetween(
        force.application_point[0], force.application_point[1],
        endX, endY
      );

      // Arrow head
      const angle = Math.atan2(force.force[1], force.force[0]);
      const arrowSize = 8;
      this.debugGraphics!.lineBetween(
        endX, endY,
        endX - Math.cos(angle - 0.3) * arrowSize,
        endY - Math.sin(angle - 0.3) * arrowSize
      );
      this.debugGraphics!.lineBetween(
        endX, endY,
        endX - Math.cos(angle + 0.3) * arrowSize,
        endY - Math.sin(angle + 0.3) * arrowSize
      );
    }
  }

  updateRTT(rtt: number) {
    this.debugInfo.rtt = rtt;
  }
}

class CosmicCrunchersClient {
  private ws: WebSocket | null = null;
  private currentRoom: string | null = null;
  private playerName: string = '';
  private serverHost: string;
  private serverPort: string;
  private serverUrl: string;
  private wsUrl: string;
  private gameScene: GameScene | null = null;
  private game: Phaser.Game | null = null;

  constructor() {
    // Get server configuration from environment variables or fall back to defaults
    this.serverHost = (import.meta.env.VITE_SERVER_HOST as string) || 'localhost';
    this.serverPort = (import.meta.env.VITE_SERVER_PORT as string) || '8080';
    this.serverUrl = `http://${this.serverHost}:${this.serverPort}`;
    this.wsUrl = `ws://${this.serverHost}:${this.serverPort}`;

    console.log(`Client configured for server: ${this.serverUrl}`);
    this.initializeGame();
  }

  private initializeGame() {
    // Create Phaser game
    const config: Phaser.Types.Core.GameConfig = {
      type: Phaser.AUTO,
      width: window.innerWidth,
      height: window.innerHeight,
      parent: 'app',
      backgroundColor: '#000011',
      physics: {
        default: 'arcade',
        arcade: {
          debug: false
        }
      },
      scene: GameScene,
      scale: {
        mode: Phaser.Scale.RESIZE,
        autoCenter: Phaser.Scale.CENTER_BOTH
      }
    };

    this.game = new Phaser.Game(config);

    // Get reference to game scene
    this.game.events.once('ready', () => {
      this.gameScene = this.game!.scene.getScene('GameScene') as GameScene;
      this.gameScene.setClient(this);
      this.showConnectionUI();
    });

    // Handle window resize
    window.addEventListener('resize', () => {
      if (this.game) {
        this.game.scale.resize(window.innerWidth, window.innerHeight);
      }
    });
  }

  private showConnectionUI() {
    // Create simple connection overlay
    const overlay = document.createElement('div');
    overlay.id = 'connectionOverlay';
    overlay.style.cssText = `
      position: fixed;
      top: 0;
      left: 0;
      width: 100%;
      height: 100%;
      background: rgba(0, 0, 0, 0.8);
      color: white;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      font-family: monospace;
      z-index: 1000;
    `;

    overlay.innerHTML = `
      <h1>🚀 Cosmic Crunchers</h1>
      <p>Phase 3 - Game Client</p>
      <div style="margin: 20px;">
        <input type="text" id="playerName" placeholder="Enter your name" 
               style="padding: 10px; margin: 10px; font-size: 16px;">
        <input type="text" id="roomCode" placeholder="Room code (optional)" 
               style="padding: 10px; margin: 10px; font-size: 16px;">
      </div>
      <div>
        <button id="connectBtn" style="padding: 10px 20px; margin: 10px; font-size: 16px;">
          Connect & Play
        </button>
        <button id="createRoomBtn" style="padding: 10px 20px; margin: 10px; font-size: 16px;">
          Create Room
        </button>
      </div>
      <div id="status" style="margin: 20px; text-align: center;"></div>
    `;

    document.body.appendChild(overlay);

    // Bind events
    document.getElementById('connectBtn')!.addEventListener('click', () => this.connectAndJoin());
    document.getElementById('createRoomBtn')!.addEventListener('click', () => this.createAndJoinRoom());
  }

  private hideConnectionUI() {
    const overlay = document.getElementById('connectionOverlay');
    if (overlay) {
      overlay.remove();
    }
  }

  private updateStatus(message: string) {
    const statusEl = document.getElementById('status');
    if (statusEl) {
      statusEl.textContent = message;
    }
  }

  private async connectAndJoin() {
    const nameInput = document.getElementById('playerName') as HTMLInputElement;
    const roomInput = document.getElementById('roomCode') as HTMLInputElement;

    this.playerName = nameInput.value.trim();
    const roomCode = roomInput.value.trim();

    if (!this.playerName) {
      this.updateStatus('Please enter a player name');
      return;
    }

    try {
      this.updateStatus('Connecting to server...');
      await this.connect();

      if (roomCode) {
        this.updateStatus('Joining room...');
        this.joinRoom(roomCode);
      } else {
        this.updateStatus('Creating room...');
        await this.createRoom();
      }
    } catch (error) {
      this.updateStatus(`Error: ${error}`);
    }
  }

  private async createAndJoinRoom() {
    const nameInput = document.getElementById('playerName') as HTMLInputElement;
    this.playerName = nameInput.value.trim();

    if (!this.playerName) {
      this.updateStatus('Please enter a player name');
      return;
    }

    try {
      this.updateStatus('Connecting to server...');
      await this.connect();
      this.updateStatus('Creating room...');
      await this.createRoom();
    } catch (error) {
      this.updateStatus(`Error: ${error}`);
    }
  }

  private async connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      this.ws = new WebSocket(`${this.wsUrl}/ws`);

      this.ws.onopen = () => {
        console.log('Connected to server');
        resolve();
      };

      this.ws.onmessage = (event) => {
        this.handleMessage(JSON.parse(event.data));
      };

      this.ws.onclose = () => {
        console.log('Disconnected from server');
        this.ws = null;
        this.currentRoom = null;
      };

      this.ws.onerror = (error) => {
        console.error('WebSocket error:', error);
        reject(error);
      };
    });
  }

  private async createRoom() {
    try {
      const response = await fetch(`${this.serverUrl}/create-room`, {
        method: 'POST'
      });
      if (response.ok) {
        const roomCode = await response.text();
        console.log(`Created room: ${roomCode}`);
        this.joinRoom(roomCode);
      } else {
        throw new Error('Failed to create room');
      }
    } catch (error) {
      throw new Error(`Failed to create room: ${error}`);
    }
  }

  private joinRoom(roomCode: string) {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      throw new Error('Not connected to server');
    }

    const message = {
      type: 'Join',
      room_code: roomCode.toUpperCase(),
      player_name: this.playerName
    };

    this.ws.send(JSON.stringify(message));
  }

  private handleMessage(message: any) {
    //console.log('Received message:', message);

    switch (message.type) {
      case 'RoomJoined':
        this.currentRoom = message.room_code;
        this.updateStatus(`Joined room: ${message.room_code}`);

        // Set local entity ID for the game scene
        if (this.gameScene) {
          this.gameScene.setLocalEntityId(message.entity_id); // Use the entity_id directly
        }

        // Hide connection UI and start game
        setTimeout(() => this.hideConnectionUI(), 1000);
        break;

      case 'PlayerJoined':
        console.log(`Player joined: ${message.player_name}`);
        break;

      case 'PlayerLeft':
        console.log(`Player left: ${message.player_id}`);
        break;

      case 'Snapshot':
        if (this.gameScene) {
          try {
            // Parse the actual snapshot data from the message.data bytes
            const snapshotBytes = new Uint8Array(message.data);
            const snapshotJson = new TextDecoder().decode(snapshotBytes);
            const snapshotData = JSON.parse(snapshotJson);
            //console.log('Received snapshot with', snapshotData.entities?.length || 0, 'entities');
            this.gameScene.handleSnapshot(snapshotData);
          } catch (error) {
            console.error('Failed to parse snapshot:', error);
            console.error('Raw message:', message);
          }
        }
        break;

      case 'Pong':
        const rtt = Date.now() - message.timestamp;
        if (this.gameScene) {
          this.gameScene.updateRTT(rtt);
        }
        break;

      case 'DebugRender':
        if (this.gameScene) {
          try {
            // Parse the debug render data
            const debugBytes = new Uint8Array(message.data);
            const debugJson = new TextDecoder().decode(debugBytes);
            const debugData = JSON.parse(debugJson);
            console.log('Received debug render data:', debugData);
            this.gameScene.handleDebugRenderData(debugData);
          } catch (error) {
            console.error('Failed to parse debug render data:', error);
            console.error('Raw message:', message);
          }
        }
        break;

      case 'Error':
        this.updateStatus(`Server error: ${message.message}`);
        break;
    }
  }

  public sendInput(input: InputData) {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;

    try {
      // Convert input to JSON bytes for server
      const inputJson = JSON.stringify(input);
      const inputBytes = Array.from(new TextEncoder().encode(inputJson));

      const message = {
        type: 'Input',
        sequence: input.sequence,
        timestamp: input.timestamp,
        data: inputBytes
      };

      this.ws.send(JSON.stringify(message));
      console.log('Sent input:', input.sequence, 'thrust:', input.thrust, 'turn:', input.turn);
    } catch (error) {
      console.error('Failed to send input:', error);
    }
  }

  public requestDebugData() {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;

    // Send a request for debug render data (you can extend this with specific debug modes later)
    const message = {
      type: 'RequestDebugRender',
      timestamp: Date.now()
    };

    this.ws.send(JSON.stringify(message));
    console.log('Requested debug render data from server');
  }

  public isConnectedToRoom(): boolean {
    return !!(this.ws && this.ws.readyState === WebSocket.OPEN && this.currentRoom);
  }
}

// Initialize the client when the page loads
new CosmicCrunchersClient();
