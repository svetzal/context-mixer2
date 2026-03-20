# Diffing with LLM Analysis

cmx can use an LLM to analyze differences between your installed artifact and the source version.

## Usage

```bash
cmx agent diff python-craftsperson
cmx skill diff blog-image-generator
```

If the artifacts are identical (checksums match), cmx reports "up to date" without calling the LLM.

When differences exist, cmx sends both versions to the configured LLM and returns a concise analysis covering:

1. What capabilities or behaviors were added, removed, or changed
2. Whether the update is significant or cosmetic
3. A recommendation on whether to update

## Example output

```
Comparing python-craftsperson (agent)
  Installed (global): unversioned
  Source (guidelines): 1.3.1

Analyzing differences...

The only difference is the addition of version metadata in the
frontmatter. No behavioral changes. Update is safe but optional.
```

## Works with untracked artifacts

`diff` works on any installed artifact that has a matching source — even ones not tracked in the lock file. It finds the file on disk and compares directly.

## LLM configuration

See [Configuration](./configuration.md) to set up the LLM gateway and model.
