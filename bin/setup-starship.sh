#!/bin/bash
# Configure Starship.rs workspace plugin for agent-workspace

# set variables with default values
STARSHIP_CONFIG="${STARSHIP_CONFIG:-$HOME/.config/starship.toml}"

set -e

echo "=== Agent Workspace Starship.rs Plugin Configuration ==="
echo ""

# Check if Starship is installed
if ! command -v starship &> /dev/null; then
    echo "Starship is not installed. Skipping configuration."
    echo "After installing Starship, run this script again to configure the workspace plugin."
    exit 0
fi

echo "✓ Starship detected"

# Find Starship config
CONFIG_DIR=$(dirname "$STARSHIP_CONFIG")

if [ ! -d "$CONFIG_DIR" ]; then
    echo "Creating config directory: $CONFIG_DIR"
    mkdir -p "$CONFIG_DIR"
fi

# Ensure starship exists
if [ ! -f "$STARSHIP_CONFIG" ]; then
    echo "Starship config not found. Skipping configuration."
    exit 1;
fi

echo "✓ Config file: $STARSHIP_CONFIG"

# Check if workspace module already exists
if grep -q "\[custom.aw\]" "$STARSHIP_CONFIG" 2>/dev/null; then
    echo "⚠️  Workspace module already configured"
    echo ""
    echo "Current configuration:"
    sed -n '/\[custom.aw\]/,/^$/p' "$STARSHIP_CONFIG"
    echo ""
    read -p "Do you want to update it? (y/N): " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Keeping existing configuration"
        exit 0
    fi
    # Remove existing workspace configuration
    sed -i.bak '/\[custom.aw\]/,/^$/d' "$STARSHIP_CONFIG"
fi

# Add workspace module
echo ""
echo "Adding workspace module to Starship config..."
cat >> "$STARSHIP_CONFIG" << 'EOF'

# Agent Workspace indicator
[custom.aw]
command = 'echo $AGENT_WORKSPACE_NAME'
when = '[ -n "$AGENT_WORKSPACE_NAME" ]'
symbol = '◉ '
style = 'fg:242'
format = '[$symbol$output]($style) '
EOF

echo "✓ Workspace module configured"

# Check if $custom is in format string
if ! grep -q '\$custom' "$STARSHIP_CONFIG"; then
    echo ""
    echo "⚠️  Note: Add '\$custom' to your format or right_format string to display the workspace."
    echo ""
    echo "Example:"
    echo '  format = "$directory$custom$character "'
fi

echo ""
echo "✓ Starship workspace plugin configured successfully"
echo ""
echo "Customization options available in: $(dirname "$0")/aw-starship-integration.examples.toml"
