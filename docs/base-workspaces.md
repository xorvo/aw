# Base Workspaces

## Overview

Agent workspace supports multiple base workspace configurations, allowing you to maintain different sets of repositories and files for different types of tasks.

## Configuration

Define multiple base configurations in your `config.yaml`:

```yaml
default:
  repos:
    - git@github.com:your-org/app.git
    - git@github.com:your-org/api.git
    - git@github.com:your-org/frontend.git
  local_files:
    - ~/projects/my-infrastructure
    - "~/projects/my-infrastructure -> infra"  # With custom destination
  description: Main development workspace

test:
  repos:
    - git@github.com:your-org/api.git
  local_files:
    - ~/projects/test-data
  description: Testing and experimentation workspace

frontend:
  repos:
    - git@github.com:your-org/frontend.git
  local_files:
    - ~/projects/design-assets
  description: Frontend-focused workspace
```

## Base Workspace Directory Structure

```
~/.agent-workspaces/
├── base/
│   ├── default/          # Default base workspace
│   │   ├── .agent-workspace/
│   │   │   └── repo-cache/  # Bare mirror caches
│   │   ├── CLAUDE.md     # Agent instructions template
│   │   └── AGENTS.md
│   ├── test/             # Test base workspace
│   └── frontend/         # Frontend base workspace
└── config.yaml
```

## Commands

### Initialize Base Workspaces

```bash
# Initialize the default base
aw init

# Initialize a specific base
aw init test
aw init frontend

# Re-initialize to update repos
aw init default
```

### Create Workspaces from Different Bases

```bash
# Create from default base
aw create my-task

# Create from specific base
aw create test-feature --base test
aw create ui-work --base frontend
aw create quick-task -b test          # Using shorthand
```

## Use Cases

### 1. Different Project Configurations

Maintain separate bases for different project types:

- `default`: Full stack with all repositories
- `backend`: Only backend services and infrastructure
- `frontend`: Only UI repositories
- `test`: Minimal setup for testing

### 2. Agent-Specific Configurations

Configure bases optimized for different use cases:

- `full`: All repositories for comprehensive tasks
- `minimal`: Lightweight base for quick tasks

### 3. Environment-Specific Bases

Separate bases for different environments:

- `staging`: Repositories configured for staging
- `development`: Full development setup
- `hotfix`: Minimal setup for quick production fixes

## Example Workflow

### Setting Up Multiple Bases

```bash
# 1. Configure your bases in config.yaml
aw edit-config

# 2. Initialize each base
aw init default
aw init test
aw init frontend

# 3. Create workspaces as needed
aw create feature-123 --base default
aw create test-456 --base test
aw create ui-789 --base frontend
```

### Working with Different Bases

```bash
# List all workspaces (shows which base each came from)
aw list

# Start working in a workspace
aw open feature-123

# Switch to a different task with different base
exit  # Leave current workspace
aw open test-456
```

## Template Files per Base

Each base can have its own template files:

```bash
# Add templates to specific base
cd ~/.agent-workspaces/base/test/
echo "Test-specific rules" > CLAUDE.md

# These templates will only be included in workspaces created from 'test' base
aw create new-test --base test
```

## Agent Instruction Files

During workspace creation, `CLAUDE.md` and `AGENTS.md` files are symlinked from the base workspace root so agents can discover project-specific instructions and edits propagate automatically.

Place these files in your base workspace:

```bash
~/.agent-workspaces/base/default/CLAUDE.md
~/.agent-workspaces/base/default/AGENTS.md
```

If only one file exists (e.g., `CLAUDE.md`), the other (`AGENTS.md`) is automatically created as a symlink so both naming conventions are supported.

## Performance Considerations

- Each base workspace is independent
- Copy-on-write (CoW) optimization works across bases
- Repositories are cached as bare mirrors in the base, then cloned locally for each workspace
- Creating workspaces from a base is fast (uses local cache as reference)

## Best Practices

1. **Keep bases focused**: Each base should serve a specific purpose
2. **Name bases clearly**: Use descriptive names that indicate their purpose
3. **Document bases**: Add description field in config.yaml
4. **Regular updates**: Periodically run `aw init <base>` to update repositories
5. **Clean up unused bases**: Remove bases you no longer need to save space

## Tmux Integration with Bases

The tmux integration works seamlessly with multiple bases:

```bash
# Start workspaces in tmux for different project types
aw create backend-fix --base backend
aw open backend-fix         # Launches in tmux session aw-backend-fix

aw create ui-feature --base frontend
aw open ui-feature          # Launches in tmux session aw-ui-feature
```

Sessions are namespaced with `aw-` prefix to avoid conflicts with regular tmux sessions.

## Troubleshooting

### Base Not Found

```bash
# If you see: "Base workspace 'xyz' not found"
# Initialize the base first:
aw init xyz
```

### Wrong Base Used

```bash
# Check which base a workspace was created from:
cat ~/agent-workspaces/my-workspace/.agent-workspace/base
```

### Updating Base Configuration

```bash
# After editing config.yaml, re-initialize the base:
aw edit-config
aw init test  # Re-initialize with new configuration
```
