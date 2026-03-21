\# Project Refactoring Plan for lenzu

Based on the successful modularization of the RemoteOCR branch into \`capture\`, \`client\`, and \`utils\`, the project is now ready for **\*\*Phase 2: The Translation HUD\*\***. This phase moves beyond simple OCR into a real-time "Closed Caption" (CC) style overlay for Furigana, Romaji, and English translations.

\#\# Recommended Project Structure

\`\`\`plaintext  
src/  
├── main.rs \# GTK Lens Entry Point (The Trigger)  
├── capture.rs \# X11 Screen capture (Verified)  
├── client.rs \# OpenRouter Multimodal Client (Updated for JSON)  
├── utils.rs \# Image processing & Base64 utilities (Verified)  
├── config.rs \# NEW: Configurable font sizes, colors, and positions  
└── overlay/ \# NEW: The "Closed Caption" HUD Logic  
 ├── mod.rs  
 └── tauri_bridge.rs \# Logic to sync with the Tauri Translucent Overlay

## **Phase 2: Implementation Strategy**

### **1\. Structured Translation (client.rs)**

To support Furigana and Translation simultaneously, the API client must move from returning a String to a structured TranslationResult.

**The Smart Prompt:** Update the OpenRouter prompt to request JSON:

"OCR the Japanese text. Return a JSON object with: 'original', 'furigana' (kanji with \[reading\]), 'romaji', and 'english'. If no Japanese is found, return an empty object with a 'debug_info' field explaining why."

**Refactor**: Implement serde::Deserialize for a TranslationResult struct to handle this multi-part response.

### **2\. The CC Overlay HUD (The "Ghost" Window)**

The goal is a non-modal, semi-opaque "Closed Caption" box that stays on screen while the Lens remains active for new captures.

**UI Requirements:**

- **Visuals**: Semi-opaque black/grey background (rgba(0,0,0,0.6)).
- **Typography**: Yellow text with black shadows (configurable font size).
- **Behavior**: Static position (Top or Bottom), but "Click-Through" so it doesn't intercept mouse events intended for the Lens or Desktop.

### **3\. Integration with Tauri (The UI Bridge)**

Since a Tauri prototype already exists for translucent rendering, we will use a **Producer-Consumer** model.

- **Producer (GTK/Rust)**: The Lens captures and sends data to OpenRouter.
- **Consumer (Tauri)**: Receives the TranslationResult and renders the CSS-styled CC box.
- **Bridge**: Use a local IPC (Unix Domain Socket or Crossbeam Channel) to pass the JSON between the Lens logic and the Tauri Frontend.

## ---

**Benefits of this Approach**

- **Persistent Context**: Users can keep the translation on screen while moving the Lens to a different part of the image.
- **Readability**: High-contrast yellow-on-dark text mimics professional subtitling.
- **Multi-View**: The HUD can display Furigana and English translation stacked, significantly aiding language learners.

## **Prioritized HUD Tasks**

### **Phase 2A: The Data Model (High Priority)**

1. **Update client.rs**: Change the prompt and implement the TranslationResult struct.
2. **Unit Testing**: Add tests to ensure the JSON parser handles empty or malformed Japanese OCR results gracefully (displaying Italicized debug info).

### **Phase 2B: The CC Overlay (Medium Priority)**

1. **Overlay Module**: Create the "Ghost" window logic.
2. **Styling**: Implement the yellow-shadowed text rendering.
3. **Configuration**: Add config.rs to allow users to toggle:
   - font_size (e.g., 18px)
   - position (Top vs Bottom)
   - translation_mode (English only, Furigana only, or Both)

### **Phase 2C: Orchestration (Low Priority)**

1. **Main Loop update**: Update the API callback in main.rs to send the results to the Overlay instead of just printing to the console.

## ---

**Updated Summary Table: New Modules**

| Module          | Difficulty | Purpose                                               |
| :-------------- | :--------- | :---------------------------------------------------- |
| **Config**      | Easy       | Handles user preferences for HUD font and position.   |
| **Client (v2)** | Medium     | Requests and parses multi-part Japanese/English JSON. |
| **Overlay**     | Hard       | Manages the transparent, click-through CC window.     |
