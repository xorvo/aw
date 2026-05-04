# Workspace Prompt Customization

The agent-workspace system can display the current workspace in your shell prompt. Here's how to customize it for different shells and themes.

## Automatic Integration

The workspace status is automatically shown in your prompt when you source the shell hook. The integration works with:

- **Powerlevel10k**: Add `agent_workspace` to your prompt elements in `~/.p10k.zsh`
- **Oh-My-Zsh**: Shows in right prompt automatically (can be disabled with `AW_DISABLE_OMZ_PROMPT=1`)
- **Starship**: Automatically skips native integration when detected (configure with setup script)
- **Native Zsh**: Shows automatically with `[◉ workspace-name]` format
- **Bash**: Prepends workspace to PS1

## Custom Integration

### Using Environment Variables

You can use these environment variables in custom prompts:

- `$AGENT_WORKSPACE_NAME`: The workspace name (if inside a workspace)
- `$AGENT_WORKSPACE`: Full path to workspace root
- `$(_aw_prompt_segment)`: Pre-formatted workspace segment

### Powerlevel10k Configuration

#### Add to Your Prompt Elements

Edit `~/.p10k.zsh` and add `agent_workspace` to your prompt elements:

```zsh
# For right prompt (recommended)
typeset -g POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS=(
  status
  command_execution_time
  agent_workspace        # Add this line
  time
)

# Or for left prompt
typeset -g POWERLEVEL9K_LEFT_PROMPT_ELEMENTS=(
  agent_workspace        # Add this line
  dir
  vcs
)
```

#### Customize Appearance

```zsh
# Customize colors and icon
typeset -g POWERLEVEL9K_AGENT_WORKSPACE_FOREGROUND=242  # Gray color
typeset -g POWERLEVEL9K_AGENT_WORKSPACE_BACKGROUND=''   # No background
typeset -g POWERLEVEL9K_AGENT_WORKSPACE_VISUAL_IDENTIFIER_EXPANSION='◉'

# Alternative styles
typeset -g POWERLEVEL9K_AGENT_WORKSPACE_FOREGROUND=39   # Blue
typeset -g POWERLEVEL9K_AGENT_WORKSPACE_VISUAL_IDENTIFIER_EXPANSION='◆'
```

See `~/.agent-workspaces/bin/aw-p10k-customization.sh` for more styling options.

### Starship Integration

#### Quick Setup

Run the setup script to automatically configure Starship:

```bash
~/.agent-workspaces/bin/setup-starship.sh
```

#### Manual Configuration

Add to your `~/.config/starship.toml`:

```toml
# Basic configuration
[custom.workspace]
command = 'echo $AGENT_WORKSPACE_NAME'
when = '[ -n "$AGENT_WORKSPACE_NAME" ]'
symbol = '◉ '
style = 'fg:242'
format = '[$symbol$output]($style) '
```

Position it in your prompt:

```toml
# Add $custom to your format string
format = "$directory$custom$git_branch$character "

# Or in right prompt
right_format = "$custom$cmd_duration$time"
```

See `~/.agent-workspaces/bin/aw-starship-integration.examples.toml` for more styling options.

### Native Zsh Custom Prompt

For complete control, disable automatic integration and use variables directly:

```zsh
# In your .zshrc
export AW_DISABLE_OMZ_PROMPT=1  # If using oh-my-zsh

# Then customize your prompt
PROMPT='${AGENT_WORKSPACE_NAME:+[◉ $AGENT_WORKSPACE_NAME] }%~ %# '
```

## Customizing the Workspace Indicator

To change the default format, modify the `_aw_prompt_segment` function in `~/.agent-workspaces/bin/aw-prompt-integration.sh`:

```bash
_aw_prompt_segment() {
    if [ -n "$AGENT_WORKSPACE_NAME" ]; then
        # Use color codes for zsh, plain text for bash
        if [ -n "$ZSH_VERSION" ]; then
            echo "%F{242}[◉ $AGENT_WORKSPACE_NAME]%f"
        else
            echo "[◉ $AGENT_WORKSPACE_NAME]"
        fi
    fi
}
```

## Troubleshooting

### Workspace Not Showing

1. Ensure the shell hook is sourced: `source ~/.agent-workspaces/bin/aw-shell-hook.sh`
2. Check that you're in a workspace: `echo $AGENT_WORKSPACE_NAME`
3. For p10k: Ensure `agent_workspace` is in your prompt elements
4. For Starship: Ensure `$custom` is in your format string

### Duplicate Workspace Display

If you see the workspace twice (e.g., when using Starship), the native integration should automatically disable itself when Starship is detected. If not:

- Reload your shell: `source ~/.zshrc`
- Or manually disable native integration by setting environment variables (see below)

### P10k Specific

- Run `~/.agent-workspaces/bin/aw-p10k-debug.sh` for diagnostics
- Manually add `agent_workspace` to `POWERLEVEL9K_LEFT_PROMPT_ELEMENTS` or `POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS` in `~/.p10k.zsh`
- Run `p10k configure` if segments aren't refreshing

## Disabling Prompt Integration

### Disable Oh-My-Zsh Integration Only

If the automatic integration conflicts with your oh-my-zsh theme:

```bash
export AW_DISABLE_OMZ_PROMPT=1
source ~/.agent-workspaces/bin/aw-shell-hook.sh
```

### Disable All Native Integration

To disable all automatic prompt integration (useful if using Starship or custom solution):

```bash
export AW_NO_PROMPT_INTEGRATION=1
source ~/.agent-workspaces/bin/aw-shell-hook.sh
```

Note: Workspace detection and environment variables will still work, only the prompt display is disabled.