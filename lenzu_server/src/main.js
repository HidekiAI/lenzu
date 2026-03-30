/* ... (previous content) ... */

// Add shutdown command handling
function handleCommand(cmd) {
  switch (cmd.type) {
    case 'message':
      if (typeof cmd.text === 'string') sendMessage(cmd.text);
      break;
    
    case 'config':
      if (cmd.settings && typeof cmd.settings === 'object') {
        applyConfigUpdate(cmd.settings);
      }
      break;
    
    case 'clear':
      if (win && !win.isDestroyed()) {
        win.webContents.send('hud:clear');
      }
      break;
    
    case 'quit':
      app.quit();
      break;
    
    case 'shutdown':
      // Handle shutdown command
      if (udpServer) {
        udpServer.close(() => {
          udpServer = null;
        });
        setTimeout(() => app.quit(), 100);
      }
      break;
    
    default:
      console.warn('[udp] Unknown command type:', cmd.type);
  }
}

// ... (
