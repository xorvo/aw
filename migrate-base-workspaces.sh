#!/bin/bash
set -e

# One-time migration script for base workspaces
# Moves *.git bare repos into .agent-workspace/repo-cache/
# Removes local_files copies from base roots (they're re-copied from source on aw create)

AW_INSTALL_DIR="${AW_INSTALL_DIR:-$HOME/.agent-workspaces}"
AW_CONFIG_FILE="${AW_CONFIG_FILE:-$AW_INSTALL_DIR/config.yaml}"
AW_BASE_DIR="$AW_INSTALL_DIR/base"

if [ ! -d "$AW_BASE_DIR" ]; then
    echo "No base workspaces found at $AW_BASE_DIR"
    exit 0
fi

echo "🔄 Migrating base workspaces to new structure..."
echo ""

for base_dir in "$AW_BASE_DIR"/*/; do
    [ -d "$base_dir" ] || continue
    base_name=$(basename "$base_dir")
    echo "=== $base_name ==="

    repo_cache="$base_dir/.agent-workspace/repo-cache"

    # Move *.git bare repos to .agent-workspace/repo-cache/
    has_bare_repos=false
    for git_dir in "$base_dir"/*.git; do
        [ -d "$git_dir" ] || continue
        has_bare_repos=true
        break
    done

    if [ "$has_bare_repos" = true ]; then
        mkdir -p "$repo_cache"
        for git_dir in "$base_dir"/*.git; do
            [ -d "$git_dir" ] || continue
            repo_name=$(basename "$git_dir")
            echo "  Moving $repo_name -> .agent-workspace/repo-cache/$repo_name"
            mv "$git_dir" "$repo_cache/$repo_name"
        done
    fi

    # Remove local_files copies from base root
    # We identify them by reading the config: for each local_files entry, compute
    # the destination name and remove it from the base root if it exists
    if command -v yq &> /dev/null && [ -f "$AW_CONFIG_FILE" ]; then
        while IFS= read -r entry; do
            [ -n "$entry" ] || continue

            # Parse "source -> dest" syntax
            if [[ "$entry" == *" -> "* ]]; then
                dest_name="${entry##* -> }"
            else
                src="${entry/#\~/$HOME}"
                dest_name=$(basename "$src")
            fi

            target="$base_dir/$dest_name"
            if [ -e "$target" ]; then
                echo "  Removing local_files copy: $dest_name"
                rm -rf "$target"
            fi
        done < <(yq -r ".${base_name}.local_files[]" "$AW_CONFIG_FILE" 2>/dev/null)
    fi

    echo "  ✓ Done"
    echo ""
done

echo "✅ Migration complete!"
echo ""
echo "Base workspace roots now only contain your template files."
echo "Bare repo caches are in .agent-workspace/repo-cache/"
echo ""
echo "You can safely delete this script now."
