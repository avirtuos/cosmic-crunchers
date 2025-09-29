#!/bin/bash

# Cosmic Crunchers - Development Start Script
# This script starts both the server and client in development mode
#
# Configuration via environment variables:
#   SERVER_HOST=0.0.0.0 SERVER_PORT=3000 CLIENT_HOST=0.0.0.0 CLIENT_PORT=8080 RUST_LOG=debug ./start.sh
#
# Or set them inline:
#   SERVER_HOST="0.0.0.0" ./start.sh  (to bind server to all interfaces)
#   CLIENT_HOST="0.0.0.0" ./start.sh  (to make Vite accessible from other machines)
#   RUST_LOG=debug ./start.sh         (to enable debug logging for server)
#   RUST_LOG=trace ./start.sh         (to enable trace logging for server)

set -e

# Configuration - Edit these to change server/client settings
SERVER_HOST=${SERVER_HOST:-"10.0.23.111"}
SERVER_PORT=${SERVER_PORT:-8080}
CLIENT_HOST=${CLIENT_HOST:-"10.0.23.111"}
CLIENT_PORT=${CLIENT_PORT:-8081}

echo "🚀 Starting Cosmic Crunchers Development Environment"
echo "=================================================="
echo "📋 Configuration:"
echo "   Server: http://${SERVER_HOST}:${SERVER_PORT}"
echo "   Client: http://${CLIENT_HOST}:${CLIENT_PORT}"
echo "=================================================="

# Function to cleanup background processes on exit
cleanup() {
    echo ""
    echo "🛑 Shutting down development servers..."
    kill $(jobs -p) 2>/dev/null || true
    exit 0
}

# Set up cleanup trap
trap cleanup INT TERM EXIT

# Check if server directory exists
if [ ! -d "server" ]; then
    echo "❌ Server directory not found. Run this script from the project root."
    exit 1
fi

# Check if client directory exists
if [ ! -d "client" ]; then
    echo "❌ Client directory not found. Run this script from the project root."
    exit 1
fi

echo "🔧 Building server..."
cd server
if ! cargo build; then
    echo "❌ Server build failed"
    exit 1
fi

echo ""
echo "🖥️  Starting server on http://${SERVER_HOST}:${SERVER_PORT}"
echo "🔗 WebSocket endpoint: ws://${SERVER_HOST}:${SERVER_PORT}/ws"
if [ -n "$RUST_LOG" ]; then
    echo "🔍 Logging level: $RUST_LOG"
fi
RUST_LOG=${RUST_LOG} COSMIC_SERVER_HOST=${SERVER_HOST} COSMIC_SERVER_PORT=${SERVER_PORT} CLIENT_HOST=${CLIENT_HOST} CLIENT_PORT=${CLIENT_PORT} cargo run &
SERVER_PID=$!
cd ..

# Wait a moment for server to start
sleep 2

echo ""
echo "🌐 Starting client development server on http://${CLIENT_HOST}:${CLIENT_PORT}"
cd client
VITE_SERVER_HOST=${SERVER_HOST} VITE_SERVER_PORT=${SERVER_PORT} npx vite --host ${CLIENT_HOST} --port ${CLIENT_PORT} &
CLIENT_PID=$!
cd ..

echo ""
echo "✅ Development environment started!"
echo ""
echo "🔗 Open your browser to: http://${CLIENT_HOST}:${CLIENT_PORT}"
echo "🎮 Use the web interface to:"
echo "   • Connect to the WebSocket server"
echo "   • Create or join a room"
echo "   • Test ping/pong and input messages"
echo ""
echo "📊 Server logs will appear below:"
echo "=================================================="

# Wait for background processes
wait
