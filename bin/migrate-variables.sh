#!/bin/bash
# Migration script to update from old variable names to new ones

echo "=== Agent Workspace Variable Migration ==="
echo ""

# Check for old variables in environment
old_vars_found=false

if [ -n "$AGENT_WORKSPACES_HOME" ]; then
    echo "⚠️  Found old variable: AGENT_WORKSPACES_HOME=$AGENT_WORKSPACES_HOME"
    echo "   New equivalent: AW_INSTALL_DIR"
    old_vars_found=true
fi

if [ -n "$AGENT_WORKSPACE_ROOT" ]; then
    echo "⚠️  Found old variable: AGENT_WORKSPACE_ROOT=$AGENT_WORKSPACE_ROOT"
    echo "   New equivalent: AW_WORKSPACES_DIR"
    old_vars_found=true
fi

if [ -n "$AGENT_WORKSPACES_CONFIG" ]; then
    echo "⚠️  Found old variable: AGENT_WORKSPACES_CONFIG=$AGENT_WORKSPACES_CONFIG"
    echo "   New equivalent: AW_CONFIG_FILE"
    old_vars_found=true
fi

if [ "$old_vars_found" = false ]; then
    echo "✓ No old environment variables found"
fi

echo ""
echo "Checking shell configuration files..."
echo ""

# Function to check and update a file
check_file() {
    local file="$1"
    local name="$2"

    if [ ! -f "$file" ]; then
        return
    fi

    local has_old=false

    if grep -q "AGENT_WORKSPACES_HOME\|AGENT_WORKSPACE_ROOT\|AGENT_WORKSPACES_CONFIG" "$file"; then
        has_old=true
        echo "📄 $name has old variable references:"
        grep -n "AGENT_WORKSPACES_HOME\|AGENT_WORKSPACE_ROOT\|AGENT_WORKSPACES_CONFIG" "$file" | sed 's/^/   Line /'
        echo ""
    fi

    if [ "$has_old" = true ]; then
        echo "   Would you like to update $name automatically? (y/N)"
        read -p "   > " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            # Backup original
            cp "$file" "$file.bak-$(date +%Y%m%d-%H%M%S)"

            # Replace old variables with new ones
            sed -i.tmp \
                -e 's/AGENT_WORKSPACES_HOME/AW_INSTALL_DIR/g' \
                -e 's/AGENT_WORKSPACE_ROOT/AW_WORKSPACES_DIR/g' \
                -e 's/AGENT_WORKSPACES_CONFIG/AW_CONFIG_FILE/g' \
                "$file"

            rm "$file.tmp"
            echo "   ✓ Updated $name"
            echo "   Backup saved with .bak extension"
        else
            echo "   Skipped $name"
        fi
        echo ""
    fi
}

# Check common shell files
check_file "$HOME/.zshrc" ".zshrc"
check_file "$HOME/.bashrc" ".bashrc"
check_file "$HOME/.bash_profile" ".bash_profile"

echo ""
echo "=== Migration Summary ==="
echo ""
echo "Old Variable Name            → New Variable Name"
echo "─────────────────────────────────────────────────"
echo "AGENT_WORKSPACES_HOME        → AW_INSTALL_DIR"
echo "AGENT_WORKSPACE_ROOT         → AW_WORKSPACES_DIR"
echo "AGENT_WORKSPACES_CONFIG      → AW_CONFIG_FILE"
echo "─────────────────────────────────────────────────"
echo ""
echo "Default Locations:"
echo "  AW_INSTALL_DIR:           ~/.agent-workspaces  (config, bases, scripts)"
echo "  AW_WORKSPACES_DIR: ~/agent-workspaces   (your workspaces)"
echo "  AW_CONFIG_FILE:    \$AW_INSTALL_DIR/config.yaml"
echo "  AW_BIN_DIR:        ~/.local/bin         (aw command)"
echo ""
echo "To set custom locations, add to your shell config:"
echo "  export AW_INSTALL_DIR=\"/custom/path/to/installation\""
echo "  export AW_WORKSPACES_DIR=\"/custom/path/to/workspaces\""
echo "  export AW_BIN_DIR=\"/custom/path/to/bin\""
echo ""

if [ "$old_vars_found" = true ] || [ -n "$(grep -l 'AGENT_WORKSPACES_HOME\|AGENT_WORKSPACE_ROOT' ~/.zshrc ~/.bashrc 2>/dev/null)" ]; then
    echo "⚠️  Action Required:"
    echo "  1. Update your shell configuration with new variable names"
    echo "  2. Reload your shell: source ~/.zshrc (or ~/.bashrc)"
    echo "  3. Re-run installation with new variables: ./install.sh"
else
    echo "✅ Your system is ready for the new variable names!"
fi
