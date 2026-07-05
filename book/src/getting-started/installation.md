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
./install.sh
```

`install.sh` installs `cmx` with the `llm` feature, installs `cmf` lean, and
refreshes zsh completions at `~/.zfunc/_cmx` when `~/.zfunc/` already exists.

## Shell completions

`cmx completions <shell>` writes the generated completion script to stdout.
Supported shells are `bash`, `zsh`, `fish`, `elvish`, and `powershell`.

Zsh:

```bash
mkdir -p ~/.zfunc
cmx completions zsh > ~/.zfunc/_cmx
```

Then add `~/.zfunc` to `fpath` and run `autoload -Uz compinit && compinit`.

Bash:

```bash
cmx completions bash | sudo tee /etc/bash_completion.d/cmx >/dev/null
```

## Verify installation

```bash
cmx --version
```
