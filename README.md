# lightignore

Lightning-fast, interactive .gitignore generator

## Features

- 📦 Automatically fetches official gitignore templates from GitHub
- 💾 Caches templates locally for offline use
- 🔧 Support for custom templates
- ⚡ Fast and lightweight

## Installation

### Using curl (Recommended)

```bash
curl -LsSf https://github.com/kmizuki/lightignore/releases/download/0.1.0/lightignore-installer.sh | sh
```

### Using cargo

```bash
cargo install lightignore
```

### From source

```bash
git clone https://github.com/kmizuki/lightignore
cd lightignore
cargo install --path .
```

## Usage

### Update template cache

```bash
lignore update
```

### List available templates

```bash
lignore list
```

### Generate .gitignore file

```bash
lignore generate
# or simply
lignore
```

### Specify output file

```bash
lignore generate -o .gitignore
```

## Custom Templates

You can define custom templates in `lignore.json`:

```json
{
  "templates": ["Rust", "Node"],
  "custom": {
    "MyProject": ["# My custom patterns", "*.local", ".env", "secrets/"]
  }
}
```

## Keyboard Shortcuts

- `Space` - Toggle template selection
- `Enter` - Confirm selection
- `Esc`/`q` - Cancel
- `↑↓←→` or `hjkl` - Navigate
- `PgUp`/`PgDn` - Scroll page
- `Home`/`End` - Jump to start/end
- `Ctrl+A` - Select all
- `Ctrl+U` - Clear all

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
