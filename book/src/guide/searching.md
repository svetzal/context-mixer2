# Searching

Search across all registered sources for agents and skills by keyword.

## Usage

```bash
cmx search <keyword>
```

The search matches against both artifact **names** and **descriptions** (case-insensitive).

## Example

```bash
$ cmx search python
  Name                    Type   Version  Source            Description
  ----------------------  -----  -------  ----------------  -----------
  python-craftsperson     agent  1.3.1    guidelines        Use this agent when writing, reviewing...
  uv-python-craftsperson  agent  1.3.1    guidelines        Use this agent when writing, reviewing...

2 result(s) found.
```

## Searching across multiple sources

Search scans every registered source. If you have both your team's marketplace and Anthropic's skills registered, results from all sources appear together:

```bash
$ cmx search pdf
  Name           Type   Version  Source            Description
  -------------  -----  -------  ----------------  -----------
  canvas-design  skill  -        anthropic-skills  Create beautiful visual art in .png and .pdf...
  pdf            skill  -        anthropic-skills  Use this skill whenever the user wants to do...

2 result(s) found.
```

## Auto-update

Search triggers auto-update for stale git-backed sources (>60 minutes since last fetch) to ensure results are current.

## After finding what you need

Install directly from the search results:

```bash
cmx skill install anthropic-skills:pdf
```
