# Listing & Status

## List installed artifacts

```bash
# Everything
cmx list

# Agents only
cmx agent list

# Skills only
cmx skill list
```

The list view shows a table with status indicators:

```
Global agents:
  Name                        Installed  Source      Available
  --------------------------  ---------  ----------  ---------
  python-craftsperson         1.3.1      guidelines  1.3.1      ✅
  rust-craftsperson           1.0.0      guidelines  2.0.0      ⚠️
  clojure-craftsperson        -          guidelines  1.0.1      ⚠️
  skill-writing               1.0.0      guidelines  1.0.0      ⛔
  my-custom-agent             -          -           -
```

### Status indicators

| Icon | Meaning |
|------|---------|
| ✅ | Up to date — installed version matches source |
| ⚠️ | Attention needed — untracked, behind, or version unknown |
| ⛔ | Deprecated — source has marked this artifact as deprecated |
| (blank) | No source available — manually installed, not from any registered source |

## Check what's outdated

```bash
cmx outdated
```

Shows only artifacts that need attention — outdated, untracked, or changed in the source. Includes artifacts on disk that match a source but were never installed through cmx.
