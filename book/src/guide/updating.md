# Updating Artifacts

## Update a single artifact

```bash
cmx agent update python-craftsperson
cmx skill update skill-creator
```

This reinstalls the artifact from its original source, updating the lock file with the new version and checksum.

## Update all tracked artifacts

```bash
cmx agent update --all
cmx skill update --all
```

Updates every artifact in the lock file that has a different checksum than the current source version.

## Adopting untracked artifacts

If you have agents or skills installed on disk that weren't installed through cmx (e.g., manually copied), use `install --all` to adopt them:

```bash
cmx agent install --all
```

This will install (and track) every source artifact that isn't already in the lock file with a matching version — including ones already on disk.
