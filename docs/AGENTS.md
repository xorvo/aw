# Instructions for AI Agents

You are working in an isolated agent workspace designed to keep your work separate from other agents and the main development environment.

## Important Information

### Current Workspace
- You are in workspace: `$AGENT_WORKSPACE_NAME`
- Workspace path: `$AGENT_WORKSPACE`
- All your work should stay within this directory

### Available Repositories
Your workspace contains clones of the repositories configured in the workspace's base configuration. Check the workspace root to see what's available.

### Command Restrictions
Certain commands may be restricted for safety based on your agent configuration. Workspace hooks can define allowed/blocked command patterns. The system will notify you if a command is blocked.

### Git Operations
- Each repository in your workspace has its own git state
- Feel free to create branches, make commits, and experiment
- Your changes won't affect other workspaces or the main repository
- Avoid force pushing or destructive git operations

### Best Practices

1. **Stay in your workspace**: All file operations should be within `$AGENT_WORKSPACE`
2. **Check before running**: Verify commands, especially those that modify state
3. **Use staging environments**: Test changes in staging before suggesting production deployment
4. **Document your changes**: Leave clear commit messages and comments
5. **Clean up**: Remove temporary files and branches when done
6. **Start with a clean working directory**: Follow the git branching practices of the project

### Getting Repository Status
To check the status of all repositories in your workspace:
```bash
for repo in $AGENT_WORKSPACE/*/; do
    if [ -d "$repo/.git" ]; then
        echo "=== $(basename $repo) ==="
        cd "$repo" && git status -s
    fi
done
```
