# cmx-core-ts

Native Bun/TypeScript port of the `cmx-core` skill-install surface.

It exposes the same embeddable shape as the Rust library:

- `ToolIdentity`
- `BundledSkill`
- `SkillInstaller`
- `ConfigPaths`
- `NodeFilesystem`
- `SystemClock`

The library scope is bundled skill installation only. It implements:

- `plan`
- `apply`
- `status`
- `remove`

Example:

```ts
import {
  BundledSkill,
  ConfigPaths,
  NodeFilesystem,
  SkillInstaller,
  SystemClock,
  ToolIdentity,
} from "cmx-core-ts";

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
