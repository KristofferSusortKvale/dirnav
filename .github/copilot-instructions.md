# Copilot Instructions: dirnav

## Build, Test, and Run

```bash
# Build the project
cargo build

# Build for release (optimized)
cargo build --release

# Run the TUI application
cargo run

# Check for errors without building
cargo check

# Run tests (if any exist)
cargo test
```

## Architecture Overview

This is a single-file TUI (Terminal User Interface) file explorer built with:
- **ratatui** (0.30.0): UI framework for rendering widgets and layouts
- **crossterm** (0.29.0): Terminal control (raw mode, events, alternate screen)
- **syntect** (5.3): Syntax highlighting for file previews
- **color-eyre** (0.6.5): Error handling with pretty backtraces

### Core Architecture Pattern

The app follows the **immediate mode GUI pattern** used by ratatui:

1. **Terminal Setup**: Put terminal in raw mode + alternate screen
2. **Event Loop**: Read input → Update state → Render UI → Repeat
3. **Double Buffering**: Ratatui draws to an internal buffer, then flushes to terminal in one go (avoids flicker)
4. **State Machine**: All UI state lives in the `App` struct (no retained widget tree)

Key point: **Ratatui doesn't own the terminal** — we just draw into a buffer and flush to stdout.

### State Management

All state is in `App` struct:
- **Directory navigation**: `cwd`, `entries` (dirs first, files sorted case-insensitively)
- **Selection**: `selected` index into `entries`
- **Preview panel**: `preview_path`, `preview_content` (cached styled lines), `preview_scroll`
- **UI toggles**: `show_hidden`, `preview_truncated`

State updates happen in response to key events, then UI is re-rendered from the updated state.

### Syntax Highlighting Integration

- Uses **lazy static initialization** (`OnceLock`) for `SyntaxSet` and `ThemeSet` to avoid repeated loading
- Manual conversion from syntect's types to ratatui's types (avoids version mismatch with `syntect-tui`)
- Preview limited to first 512 KB of file content
- Binary file detection: if >25% of bytes are non-printable, show "(binary file)"

### Layout Structure

Vertical split into 3 sections (see `ui()` function):
1. **Path bar** (height: 3): Shows current directory + hidden file indicator
2. **Main area** (flexible): 
   - File list only, OR
   - Split horizontally: file list (50%) | preview panel (50%)
3. **Key hints bar** (height: 3): Shows available keybindings

## Key Conventions

### File Reading and Preview

- **Always check if file is binary** before attempting UTF-8 decode
- Preview panel uses **cached styled lines** stored in `preview_content` (don't re-highlight on every frame)
- Scroll position clamped to `content.len() - viewport_height` to prevent scrolling past end

### Directory Sorting

- **Directories always come first**, then files
- Both groups sorted **case-insensitively** (`to_lowercase()`)
- Special case: `".."` always appears first (if not at root)

### Key Event Handling

- **Only respond to `KeyEventKind::Press`** (ignore repeat/release) to avoid rapid selection movement
- **Dual-purpose keys**:
  - `j`/`k`: Selection navigation when no preview open; scroll preview when open
  - `Esc`: Close preview if open; quit app if no preview
  
### Error Handling

- Uses `color_eyre` for main error handling
- File operations use `Result` and show error messages in UI (e.g., "Error reading: ...")
- Directory read failures return empty `Vec<DirEntry>` rather than crashing

### Terminal Restoration

Always pair setup/teardown:
```rust
enable_raw_mode() / disable_raw_mode()
EnterAlternateScreen / LeaveAlternateScreen
terminal.show_cursor() at end
```

This ensures the terminal is properly restored even if the app panics (thanks to `color_eyre`).
