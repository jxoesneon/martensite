# Dockerfile for testing martensite-shell Linux/Wayland backends.
#
# Provides:
#   - Rust stable toolchain
#   - D-Bus session bus (dbus-daemon)
#   - A minimal StatusNotifierWatcher implementation for SNI tests
#   - Headless weston compositor for fractional-scale tests (optional)
#
# Usage:
#   docker build -t martensite-shell-test -f docker/shell-test.Dockerfile .
#   docker run --rm martensite-shell-test
#
# The container runs the shell integration tests with --ignored to
# exercise the D-Bus-dependent SNI registration.

FROM rust:1.95-slim-bookworm

# Install system dependencies:
# - dbus: session bus for SNI registration tests
# - weston: headless Wayland compositor (optional, for fractional-scale)
# - pkg-config, libdbus-1-dev: for zbus compilation
RUN apt-get update && apt-get install -y --no-install-recommends \
    dbus \
    dbus-x11 \
    weston \
    pkg-config \
    libdbus-1-dev \
    libglib2.0-dev \
    libcairo2-dev \
    libpango1.0-dev \
    libvulkan1 \
    mesa-vulkan-drivers \
    libegl1 \
    clang \
    gcc \
    mold \
    python3 \
    python3-dbus \
    python3-gi \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

# Copy the workspace. In CI this is done via volume mount; for a
# standalone build, copy the entire workspace.
COPY . .

# Build the shell crate with the wayland-backend feature.
RUN cargo build -p martensite-shell --features wayland-backend

# Script to start a D-Bus session and run the tests.
RUN cat > /run-tests.sh << 'SCRIPT'
#!/bin/bash
set -euo pipefail

# Start a D-Bus session bus.
eval "$(dbus-launch --sh-syntax)"
export DBUS_SESSION_BUS_ADDRESS
echo "D-Bus session bus: $DBUS_SESSION_BUS_ADDRESS"

# Start a minimal StatusNotifierWatcher stub on the session bus.
python3 -c "
import dbus, dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib
DBusGMainLoop(set_as_default=True)
class Watcher(dbus.service.Object):
    def __init__(self, bus):
        bus_name = dbus.service.BusName('org.kde.StatusNotifierWatcher', bus=bus)
        dbus.service.Object.__init__(self, bus_name, '/StatusNotifierWatcher')
    @dbus.service.method('org.kde.StatusNotifierWatcher', in_signature='s')
    def RegisterStatusNotifierItem(self, service):
        print(f'Registered: {service}')
bus = dbus.SessionBus()
Watcher(bus)
loop = GLib.MainLoop()
loop.run()
" &
WATCHER_PID=$!
sleep 2

# Run the shell integration tests (including ignored D-Bus tests).
cargo test -p martensite-shell --features wayland-backend --test shell_integration -- --ignored

# Clean up.
kill $WATCHER_PID 2>/dev/null || true
SCRIPT
RUN chmod +x /run-tests.sh

CMD ["/run-tests.sh"]
