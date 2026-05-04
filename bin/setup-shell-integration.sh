#!/bin/bash
# Setup script to add agent-workspace shell integration

# set variables with default values
AW_INSTALL_DIR="${AW_INSTALL_DIR:-$HOME/.agent-workspaces}"
SHELL_HOOK='[ -f "$AW_INSTALL_DIR/bin/aw-shell-hook.sh" ] && source "$AW_INSTALL_DIR/bin/aw-shell-hook.sh"'
COMPLETION_HOOK='[ -f "$AW_INSTALL_DIR/bin/aw-completion.sh" ] && source "$AW_INSTALL_DIR/bin/aw-completion.sh"'

echo "🔧 Setting up shell integration for agent-workspace..."
echo ""

# Function to add hook to a shell RC file
add_to_rc() {
    local rc_file="$1"
    local shell_name="$2"

    if [ -f "$rc_file" ]; then
        # Check if already added
        if grep -q "aw-shell-hook.sh" "$rc_file"; then
            echo "✓ $shell_name already configured"
        else
            echo "" >> "$rc_file"
            echo "# Agent Workspace integration" >> "$rc_file"
            echo "export AW_INSTALL_DIR=\"$AW_INSTALL_DIR\"" >> "$rc_file"
            echo "$SHELL_HOOK" >> "$rc_file"
            echo "$COMPLETION_HOOK" >> "$rc_file"
            echo "✓ Added to $shell_name"
        fi
    else
        echo "⚠️  $rc_file not found, skipping $shell_name"
    fi
}

# Add to various shell configs
add_to_rc "$HOME/.zshrc" "Zsh"
add_to_rc "$HOME/.bashrc" "Bash"
add_to_rc "$HOME/.bash_profile" "Bash (profile)"

echo ""
echo "✅ Shell integration setup complete!"
echo ""
echo "The following lines have been added to your shell configuration:"
echo "  export AW_INSTALL_DIR=\"$AW_INSTALL_DIR\""
echo "  $SHELL_HOOK"
echo "  $COMPLETION_HOOK"
echo ""
echo "This enables:"
echo "  • Auto-activation: Automatically activates workspaces when you cd into them"
echo "  • Tab completion: Complete base names, workspace names, and command options"
echo "  • Environment variables: Sets AGENT_WORKSPACE environment variables automatically"
echo "  • Custom hooks: Sources hooks.d scripts for command wrappers and env vars"
echo "  • Prompt integration: Shows workspace in your shell prompt (zsh/bash/p10k)"
echo ""

# Check for Starship and offer to configure
if command -v starship &> /dev/null; then
    echo "📍 Starship detected. Would you like to configure the workspace plugin?"
    read -p "Configure Starship workspace plugin? (y/N): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        "$AW_INSTALL_DIR/bin/setup-starship.sh"
    fi
    echo ""
fi

echo "To activate in your current shell, run:"
echo "  source ~/.zshrc  # for Zsh"
echo "  source ~/.bashrc # for Bash"
echo ""
echo "Next steps:"
echo "  1. Source your shell config or restart your terminal"
echo "  2. Run 'aw init' to initialize base workspace"
echo "  3. Run 'aw create <name>' to create a workspace"
echo "  4. Run 'aw start <name>' to enter workspace"
