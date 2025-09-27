import './style.css'

// WebSocket client for Cosmic Crunchers
class CosmicCrunchersClient {
  private ws: WebSocket | null = null;
  private currentRoom: string | null = null;
  private playerId: string | null = null;
  private playerName: string = '';
  private serverHost: string;
  private serverPort: string;
  private serverUrl: string;
  private wsUrl: string;

  constructor() {
    // Get server configuration from environment variables or fall back to defaults
    this.serverHost = (import.meta.env.VITE_SERVER_HOST as string) || 'localhost';
    this.serverPort = (import.meta.env.VITE_SERVER_PORT as string) || '8080';
    this.serverUrl = `http://${this.serverHost}:${this.serverPort}`;
    this.wsUrl = `ws://${this.serverHost}:${this.serverPort}`;
    
    console.log(`Client configured for server: ${this.serverUrl}`);
    this.setupUI();
  }

  private setupUI() {
    document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
      <div class="container">
        <h1>🚀 Cosmic Crunchers</h1>
        <p>Multiplayer Space Shooter - Phase 1 Testing</p>
        
        <div class="connection-section">
          <h2>Connection</h2>
          <div class="input-group">
            <label for="playerName">Player Name:</label>
            <input type="text" id="playerName" placeholder="Enter your name" />
          </div>
          <button id="connectBtn">Connect to Server</button>
          <div id="connectionStatus" class="status">Disconnected</div>
        </div>

        <div class="room-section">
          <h2>Room Management</h2>
          <div class="create-room-area">
            <button id="createRoomBtn" disabled>Create New Room</button>
            <div id="createdRoomDisplay" class="created-room" style="display: none;">
              <strong>Created Room: <span id="createdRoomCode"></span></strong>
            </div>
          </div>
          <div class="join-room-area">
            <div class="input-group">
              <label for="roomCode">Join Existing Room:</label>
              <input type="text" id="roomCode" placeholder="Enter 8-character room code" maxlength="8" />
            </div>
            <button id="joinRoomBtn" disabled>Join Room</button>
          </div>
          <div class="button-group">
            <button id="leaveRoomBtn" disabled>Leave Room</button>
          </div>
          <div id="roomStatus" class="status">Not in a room</div>
        </div>

        <div class="test-section">
          <h2>Phase 1 Testing</h2>
          <div class="button-group">
            <button id="pingBtn" disabled>Send Ping</button>
            <button id="testInputBtn" disabled>Send Test Input</button>
          </div>
          <div id="latency" class="status">Latency: --ms</div>
        </div>

        <div class="log-section">
          <h2>Message Log</h2>
          <div id="messageLog" class="message-log"></div>
          <button id="clearLogBtn">Clear Log</button>
        </div>
      </div>
    `;

    this.bindEvents();
  }

  private bindEvents() {
    const connectBtn = document.getElementById('connectBtn') as HTMLButtonElement;
    const createRoomBtn = document.getElementById('createRoomBtn') as HTMLButtonElement;
    const joinRoomBtn = document.getElementById('joinRoomBtn') as HTMLButtonElement;
    const leaveRoomBtn = document.getElementById('leaveRoomBtn') as HTMLButtonElement;
    const pingBtn = document.getElementById('pingBtn') as HTMLButtonElement;
    const testInputBtn = document.getElementById('testInputBtn') as HTMLButtonElement;
    const clearLogBtn = document.getElementById('clearLogBtn') as HTMLButtonElement;

    connectBtn.addEventListener('click', () => this.toggleConnection());
    createRoomBtn.addEventListener('click', () => this.createRoom());
    joinRoomBtn.addEventListener('click', () => this.joinRoom());
    leaveRoomBtn.addEventListener('click', () => this.leaveRoom());
    pingBtn.addEventListener('click', () => this.sendPing());
    testInputBtn.addEventListener('click', () => this.sendTestInput());
    clearLogBtn.addEventListener('click', () => this.clearLog());

    // Auto-connect on Enter key
    document.getElementById('playerName')?.addEventListener('keypress', (e) => {
      if (e.key === 'Enter' && !this.ws) {
        this.toggleConnection();
      }
    });

    document.getElementById('roomCode')?.addEventListener('keypress', (e) => {
      if (e.key === 'Enter' && this.ws && !this.currentRoom) {
        this.joinRoom();
      }
    });
  }

  private async toggleConnection() {
    if (this.ws) {
      this.disconnect();
    } else {
      await this.connect();
    }
  }

  private async connect() {
    const playerNameInput = document.getElementById('playerName') as HTMLInputElement;
    this.playerName = playerNameInput.value.trim();
    
    if (!this.playerName) {
      this.log('Please enter a player name', 'error');
      return;
    }

    try {
      this.ws = new WebSocket(`${this.wsUrl}/ws`);
      
      this.ws.onopen = () => {
        this.log(`Connected to server at ${this.serverUrl}`, 'success');
        this.updateConnectionStatus('Connected', true);
        this.updateButtonStates();
      };

      this.ws.onmessage = (event) => {
        this.handleMessage(JSON.parse(event.data));
      };

      this.ws.onclose = () => {
        this.log('Disconnected from server', 'warning');
        this.updateConnectionStatus('Disconnected', false);
        this.ws = null;
        this.currentRoom = null;
        this.playerId = null;
        this.updateButtonStates();
        this.updateRoomStatus('Not in a room');
      };

      this.ws.onerror = (error) => {
        this.log(`Connection error: ${error}`, 'error');
      };

    } catch (error) {
      this.log(`Failed to connect: ${error}`, 'error');
    }
  }

  private disconnect() {
    if (this.ws) {
      if (this.currentRoom) {
        this.leaveRoom();
      }
      this.ws.close();
    }
  }

  private async createRoom() {
    try {
      const response = await fetch(`${this.serverUrl}/create-room`, {
        method: 'POST'
      });
      if (response.ok) {
        const roomCode = await response.text();
        this.log(`Created room: ${roomCode}`, 'success');
        
        // Show the created room code prominently
        this.showCreatedRoom(roomCode);
        
        // Put the code in the input field for easy copying
        (document.getElementById('roomCode') as HTMLInputElement).value = roomCode;
        
        // Auto-join the created room after a brief delay
        setTimeout(() => this.joinRoom(), 1000);
      } else {
        this.log('Failed to create room', 'error');
      }
    } catch (error) {
      this.log(`Failed to create room: ${error}`, 'error');
    }
  }

  private showCreatedRoom(roomCode: string) {
    const createdRoomDisplay = document.getElementById('createdRoomDisplay')!;
    const createdRoomCode = document.getElementById('createdRoomCode')!;
    
    createdRoomCode.textContent = roomCode;
    createdRoomDisplay.style.display = 'block';
    
    // Hide the display after joining the room
    setTimeout(() => {
      createdRoomDisplay.style.display = 'none';
    }, 3000);
  }

  private joinRoom() {
    const roomCodeInput = document.getElementById('roomCode') as HTMLInputElement;
    const roomCode = roomCodeInput.value.trim().toUpperCase();
    
    if (!roomCode) {
      this.log('Please enter a room code', 'error');
      return;
    }

    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      this.log('Not connected to server', 'error');
      return;
    }

    const message = {
      type: 'Join',
      room_code: roomCode,
      player_name: this.playerName
    };

    this.ws.send(JSON.stringify(message));
    this.log(`Attempting to join room: ${roomCode}`, 'info');
  }

  private leaveRoom() {
    if (!this.ws || !this.currentRoom) return;

    const message = { type: 'Leave' };
    this.ws.send(JSON.stringify(message));
    this.log('Left room', 'info');
  }

  private sendPing() {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;

    const timestamp = Date.now();
    const message = {
      type: 'Ping',
      timestamp
    };

    this.ws.send(JSON.stringify(message));
    this.log(`Sent ping at ${timestamp}`, 'info');
  }

  private sendTestInput() {
    if (!this.ws || !this.currentRoom) return;

    const message = {
      type: 'Input',
      sequence: Math.floor(Math.random() * 1000),
      timestamp: Date.now(),
      data: new Array(10).fill(0).map(() => Math.floor(Math.random() * 256))
    };

    this.ws.send(JSON.stringify(message));
    this.log(`Sent test input (seq: ${message.sequence})`, 'info');
  }

  private handleMessage(message: any) {
    this.log(`Received: ${JSON.stringify(message)}`, 'received');

    switch (message.type) {
      case 'RoomJoined':
        this.currentRoom = message.room_code;
        this.playerId = message.player_id;
        this.updateRoomStatus(`In room: ${message.room_code}`);
        this.log(`Joined room ${message.room_code} as ${message.player_id}`, 'success');
        this.updateButtonStates();
        break;

      case 'PlayerJoined':
        this.log(`Player joined: ${message.player_name} (${message.player_id})`, 'info');
        break;

      case 'PlayerLeft':
        this.log(`Player left: ${message.player_id}`, 'info');
        break;

      case 'Snapshot':
        this.log(`Received snapshot (seq: ${message.sequence})`, 'info');
        break;

      case 'Pong':
        const latency = Date.now() - message.timestamp;
        this.updateLatency(latency);
        this.log(`Pong received, latency: ${latency}ms`, 'success');
        break;

      case 'Error':
        this.log(`Server error: ${message.message}`, 'error');
        break;

      default:
        this.log(`Unknown message type: ${message.type}`, 'warning');
    }
  }

  private updateConnectionStatus(status: string, connected: boolean) {
    const statusEl = document.getElementById('connectionStatus')!;
    statusEl.textContent = status;
    statusEl.className = `status ${connected ? 'connected' : 'disconnected'}`;
  }

  private updateRoomStatus(status: string) {
    const statusEl = document.getElementById('roomStatus')!;
    statusEl.textContent = status;
  }

  private updateLatency(latency: number) {
    const latencyEl = document.getElementById('latency')!;
    latencyEl.textContent = `Latency: ${latency}ms`;
  }

  private updateButtonStates() {
    const isConnected = this.ws && this.ws.readyState === WebSocket.OPEN;
    const inRoom = !!this.currentRoom;

    (document.getElementById('connectBtn') as HTMLButtonElement).textContent = 
      isConnected ? 'Disconnect' : 'Connect to Server';
    
    (document.getElementById('createRoomBtn') as HTMLButtonElement).disabled = !isConnected || inRoom;
    (document.getElementById('joinRoomBtn') as HTMLButtonElement).disabled = !isConnected || inRoom;
    (document.getElementById('leaveRoomBtn') as HTMLButtonElement).disabled = !inRoom;
    (document.getElementById('pingBtn') as HTMLButtonElement).disabled = !isConnected;
    (document.getElementById('testInputBtn') as HTMLButtonElement).disabled = !isConnected || !inRoom;
  }

  private log(message: string, type: 'info' | 'success' | 'warning' | 'error' | 'received' = 'info') {
    const logEl = document.getElementById('messageLog')!;
    const timestamp = new Date().toLocaleTimeString();
    const logEntry = document.createElement('div');
    logEntry.className = `log-entry ${type}`;
    logEntry.textContent = `[${timestamp}] ${message}`;
    logEl.appendChild(logEntry);
    logEl.scrollTop = logEl.scrollHeight;
  }

  private clearLog() {
    document.getElementById('messageLog')!.innerHTML = '';
  }
}

// Initialize the client when the page loads
new CosmicCrunchersClient();
