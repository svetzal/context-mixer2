# cmx-core (TypeScript)

Native Bun/TypeScript port of the `cmx-core` artifact-install surface. Published to
npm as `cmx-core`; the source lives in the `cmx-core-ts/` directory of the
[context-mixer2](https://github.com/svetzal/context-mixer2) repo, alongside the
Rust reference and the shared conformance fixtures.

It exposes the same embeddable shape as the Rust library:

- `ToolIdentity`
- `BundledSkill`
- `SkillInstaller`
- `ArtifactIdentity`
- `BundledArtifact`
- `ArtifactInstaller`
- `ConfigPaths`
- `NodeFilesystem`
- `SystemClock`

The library installs generated agents and bundled skills. It implements:

- `plan`
- `apply`
- `status`
- `remove`

Generated agents use Markdown as their canonical form. The installer adapts
that source to each target, including Codex's TOML subagent format, while
tracking both the canonical source checksum and installed checksum.

Example:

```ts
import {
  BundledSkill,
  ConfigPaths,
  NodeFilesystem,
  SkillInstaller,
  SystemClock,
  ToolIdentity,
} from "cmx-core";

const installer = new SkillInstaller(new ToolIdentity("mytool", "1.2.0"));
const skill = BundledSkill.singleMd("---\nname: mytool\n---\n# My skill\n");
const context = {
  fs: new NodeFilesystem(),
  clock: new SystemClock(),
  paths: ConfigPaths.fromEnv("claude"),
};

const plan = await installer.plan(skill, "global", false, context);
const report = await installer.apply(skill, plan, context);
```

## Conformance

Run the full fixture suite from this package directory:

```bash
bun install
bun test
bunx tsc --noEmit
bunx biome check .
```

The test harness consumes the committed fixtures in `../cmx-core/conformance/` and checks:

- checksum parity
- byte-exact frontmatter reconciliation
- version-guard decisions
- platform paths and lock names
- target resolution
- end-to-end install behavior, tree snapshots, lock JSON values, and normalized reports
- generated-agent transformation and checksum parity
