# Performance Considerations

## Large Git Repository Handling

When working with large git repositories, if you network condition is good. It's actually recommended to clone from remote. It will be extreamly slow if you just do `cp -R source dest`.

In fact, it is recommended to use git clone for local git repositories. The performance difference is night and day. Thanks to git's optimizations.

```
git clone file:///path/to/original /path/to/copy
```

## More Details

### The Challenge

Some repositories can be quite large. For example:
- **Large file count**: Repositories with 50,000+ files
- **Large size**: Repositories over 1GB
- **Complex structure**: Many symlinks, nested directories, or build artifacts

Even with copy-on-write (CoW) optimization on modern filesystems like APFS, creating workspaces from such repositories can take several minutes.
- CoW still needs to create metadata entries for each file
- Directory structures need to be replicated
- File permissions and attributes must be preserved

### Other Optimizations

For non-git but large directories.

We implemented a mechanism to automatically uses the fastest available copy method:

1. **Copy-on-Write (macOS APFS)**: Uses `cp -c` for instant* cloning
2. **Reflink (Linux Btrfs/XFS)**: Uses `cp --reflink=auto` for instant* cloning
3. **Standard copy**: Falls back to regular `cp -R`
