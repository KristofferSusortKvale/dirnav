# dirnav

A lightweight, fast TUI (Terminal User Interface) file explorer built with Rust, featuring syntax-highlighted file previews and rendered markdown viewing.

## Features

- **Fast directory navigation** with vim-like keybindings
- **Syntax-highlighted file previews** powered by [syntect](https://github.com/trishume/syntect)
- **Rendered markdown preview** - view `.md` files with proper formatting, styled headings, code blocks, lists, and more
- **Toggle between raw and rendered** views for markdown files
- **Hidden files toggle** - show or hide dotfiles
- **Smooth scrolling** in preview panel
- **Clean, minimal interface** built with [ratatui](https://github.com/ratatui/ratatui)

## Installation

### Build from source

Requires Rust 1.91.1 or later.

```bash
git clone https://github.com/KristofferSusortKvale/dirnav.git
cd dirnav
cargo build --release
```

The binary will be available at `target/release/dirnav`.

Optionally, install it to your system:

```bash
cargo install --path .
```

## Usage

```bash
dirnav
```

The application will start in your current directory.

### Keybindings

| Key | Action |
|-----|--------|
| `↑` / `k` | Move selection up |
| `↓` / `j` | Move selection down |
| `Enter` / `l` | Open directory or preview file |
| `h` | Go to parent directory |
| `H` | Toggle hidden files visibility |
| `t` | Toggle between raw/rendered view (markdown files only) |
| `j` / `k` | Scroll preview up/down (when preview is open) |
| `Esc` | Close preview (or quit if no preview open) |
| `q` | Quit |

## Markdown Preview

When you open a `.md` file, dirnav automatically renders it with:

- **Styled headings** - different colors and formatting for H1-H6
- **Syntax-highlighted code blocks** - detects language and applies appropriate highlighting
- **Formatted lists** - proper indentation and bullet points
- **Blockquotes** - with visual prefix
- **Emphasis and strong text** - italic and bold styling
- **Inline code** - distinctive styling
- **Horizontal rules**

Press `t` while viewing a markdown file to toggle between the rendered view and the raw syntax-highlighted source.

## Technical Details

Built with:
- [ratatui](https://github.com/ratatui/ratatui) - Terminal UI framework
- [crossterm](https://github.com/crossterm-rs/crossterm) - Terminal manipulation
- [syntect](https://github.com/trishume/syntect) - Syntax highlighting
- [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark) - Markdown parsing
- [color-eyre](https://github.com/eyre-rs/color-eyre) - Error handling

The application uses an immediate mode GUI pattern with event-driven updates, ensuring minimal resource usage and fast rendering.

## Preview Limits

- File previews are limited to the first 512 KB
- Binary files are automatically detected and skipped
- Files must be valid UTF-8 for preview

## License

MIT

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
