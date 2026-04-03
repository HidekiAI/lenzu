# Shutdown Plan for lenzu Client and Server

## Current Issue

When the lenzu client is closed (e.g., by pressing Esc), the lenzu server process does not terminate properly. This leaves the server running in the background, which can cause resource leaks and unexpected behavior.

## Root Cause Analysis

- The client's `kill_server` function is called when Esc is pressed, but it only attempts to kill the server process through the existing `server_process` reference.
- However, the server may not be properly listening for shutdown commands or may not be designed to handle termination signals gracefully.
- The current implementation may not be capturing the server's PID for direct termination.

## Proposed Solutions

### Option 1: UDP Shutdown Command

1. **Server Modification**:
    - Add a UDP listener in the server to detect a "shutdown" command.
    - When received, the server should gracefully terminate.

2. **Client Modification**:
    - Modify the `kill_server` function to send a "shutdown" UDP message to the server before exiting.
    - Ensure the message is sent to the correct port.

### Option 2: PID Tracking

1. **Server Modification**:
    - When starting the server, capture its PID.
    - Store the PID in the AppState for later use.

2. **Client Modification**:
    - When exiting, send a SIGTERM signal to the stored PID.
    - Ensure proper cleanup of resources.

## Recommendation
Option 1 is preferred as it provides a clean, message-based shutdown mechanism that's easier to test and debug. It also doesn't rely on OS-specific signal handling, which can be platform-dependent.

## Next Steps

- Implement Option 1 by modifying both client and server code.
- Test the shutdown functionality to ensure the server terminates properly when the client exits.
