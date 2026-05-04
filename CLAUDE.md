# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Agent Workspace (`aw`) is a CLI tool that manages isolated working directories for AI agents like Claude Code. It enables multiple agents to work simultaneously without interfering with each other or the main development environment.

## Architecture

The entire CLI is implemented in a single embedded bash script within `install.sh`. During installation, this script writes the `aw` command to `$AW_BIN_DIR` (default: `~/.local/bin/aw`).

**Key components:**
- **install.sh** (lines 32-800): Contains the embedded `aw` CLI tool that gets written during installation
- **bin/**: Shell integration scripts (completion, prompt, hooks)
- **config.example.yaml**: Template configuration with base workspace definitions

**Directory structure created by installation:**
- `~/.agent-workspaces/` (`AW_INSTALL_DIR`): Configs, base workspaces, scripts
- `~/agent-workspaces/` (`AW_WORKSPACES_DIR`): Created workspaces
- `~/.local/bin/aw` (`AW_BIN_DIR`): CLI binary

## Common Commands

```bash
# Install/reinstall the CLI
./install.sh

# Test the CLI after changes (install.sh must be run first to update aw binary)
aw help
aw init           # Initialize base workspace
aw create test-ws # Create workspace
aw list           # List workspaces
aw delete test-ws # Delete workspace
```

## Development Workflow

1. Make changes to the embedded CLI in `install.sh` (between `cat > "$AW_BIN_DIR/$CLI_NAME" << 'EOF'` and `EOF`)
2. Run `./install.sh` to update the installed `aw` command
3. Test changes with `aw` commands

## Dependencies

- **yq**: Required for YAML parsing (checked in `check_dependencies()`)
- **git**: For repository cloning
- **tmux** (optional): For `--tmux` option

## Key Environment Variables

| Variable | Purpose |
|----------|---------|
| `AGENT_WORKSPACE` | Current workspace directory |
| `AGENT_WORKSPACE_NAME` | Workspace name |
| `AW_INSTALL_DIR` | Installation directory |
| `AW_WORKSPACES_DIR` | Workspaces directory |
| `AW_CONFIG_FILE` | Config file path |
