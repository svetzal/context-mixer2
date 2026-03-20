# Installation

## Homebrew (macOS and Linux)

```bash
brew tap svetzal/tap
brew install cmx
```

## From GitHub Releases

Download the latest binary for your platform from [GitHub Releases](https://github.com/svetzal/context-mixer2/releases).

| Platform | Archive |
|----------|---------|
| macOS (Apple Silicon) | `cmx-darwin-arm64.tar.gz` |
| macOS (Intel) | `cmx-darwin-x64.tar.gz` |
| Linux (x64) | `cmx-linux-x64.tar.gz` |

Extract and place `cmx` somewhere on your `PATH`:

```bash
tar xzf cmx-darwin-arm64.tar.gz
sudo mv cmx /usr/local/bin/
```

## From source

```bash
git clone https://github.com/svetzal/context-mixer2.git
cd context-mixer2
cargo install --path .
```

## Verify installation

```bash
cmx --version
```
