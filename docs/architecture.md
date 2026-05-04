# Agent Workspace Architecture

## Overview

Agent Workspace is a CLI tool designed to manage isolated working directories for AI agents (like Claude Code). It allows multiple agents to work simultaneously without interfering with each other or the developer's main workspace.

## Core Concepts

### Workspaces

A workspace is a complete copy of your project repositories where an AI agent can work independently. Each workspace:

- Contains clones of all configured repositories
- Has its own git state and working tree
- Is isolated from other workspaces and the main development environment
- Can be created, started, and deleted independently

## Components

### 1. CLI Tool (`aw`)

The main command-line interface providing:

- `init`: Initialize the base workspace (clone/cache repos)
- `create`: Create new workspaces from a base
- `open`: Activate a workspace (sets environment, optionally in tmux)
- `list`: Show all available workspaces
- `delete`: Remove workspaces
- `sync`: Fast-forward workspace repos with remote
- `config`: Display configuration

### 2. Hooks System (`hooks.d`)

An extensible hook system for customizing workspace behavior:

- **Global hooks** (`~/.agent-workspaces/hooks.d/*.sh`): Apply to all workspaces
- **Per-workspace hooks** (`<workspace>/.agent-workspace/hooks.d/*.sh`): Apply to a specific workspace

Hooks are sourced on workspace activation and can define command wrappers, set environment variables, or apply safety restrictions.

### 3. Configuration System (`config.yaml`)

Defines:

- **Base configurations**: Named workspace templates with repos and local files
- **Repository list**: Git repositories to clone
- **Local files**: Directories/files to copy, with optional destination mapping (`source -> dest`)
- **Agent-specific settings**: Allowed/blocked command patterns, environment restrictions (used by hooks)

## Security Model

### Workspace Isolation

- Each workspace operates in its own directory tree
- No shared state between workspaces
- Agents cannot access files outside their workspace

### Custom Command Restrictions

Hooks can enforce command restrictions per agent type:

- **Allowed commands**: Regex patterns for permitted operations
- **Blocked commands**: Patterns that are always rejected
- **Environment restrictions**: Prevents access to production/sensitive environments

### Audit Trail

Hooks can log commands for audit purposes.

## Environment Variables

| Variable                  | Description                       | Example                                               |
| ------------------------- | --------------------------------- | ----------------------------------------------------- |
| `AGENT_WORKSPACE`         | Current workspace directory       | `/home/user/agent-workspaces/workspace-1`             |
| `AGENT_WORKSPACE_NAME`    | Workspace name                    | `workspace-1`                                         |
| `AW_WORKSPACES_DIR`       | Directory where workspaces are created | `~/agent-workspaces`                             |
| `AW_BIN_DIR`              | Where aw command is installed     | `~/.local/bin`                                        |
| `AW_INSTALL_DIR`          | Installation directory            | `~/.agent-workspaces`                                 |
| `AW_CONFIG_FILE`          | Configuration file path           | `$AW_INSTALL_DIR/config.yaml`                         |

## Workflow

1. **Installation**: Run `install.sh` to set up the CLI tool
2. **Configuration**: Edit `config.yaml` to specify repositories and settings
3. **Initialization**: Run `aw init` to create the base workspace (caches repos)
4. **Workspace Creation**: `aw create task-1` creates a new isolated environment
5. **Activation**: `aw open task-1` sets up environment variables and hooks
6. **Agent Work**: AI agent operates within the workspace
7. **Cleanup**: `aw delete task-1` removes the workspace

## Best Practices

### Repository Management

- Keep the base workspace up-to-date by periodically reinitializing
- Use `aw sync` to fast-forward repos in active workspaces

### Security

- Use hooks to enforce command restrictions for your environment
- Never allow production environment access in agent workspaces
- Use separate configurations for different agent trust levels

### Performance

- Create workspaces on local SSDs for best performance
- Clean up unused workspaces regularly

## Extensibility

The architecture supports extension through:

1. **Custom Hooks**: Add shell scripts to `hooks.d/` for command wrappers, env vars, safety checks
2. **Multiple Bases**: Define different workspace templates for different workflows
3. **Agent Profiles**: Define multiple configuration profiles in `agent_config`
4. **Shell Integration**: Auto-activation, prompt indicators, tab completion
