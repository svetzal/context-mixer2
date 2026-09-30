import { describe, expect, test } from "bun:test";

import { ArtifactIdentity, ArtifactInstaller, BundledArtifact } from "../src/artifact-installer.ts";
import { saveConfig } from "../src/config.ts";
import { FixedClock, makeTempFilesystem } from "./helpers.ts";

describe("ArtifactInstaller", () => {
  for (const kind of ["agent", "skill"] as const) {
    test(`explicit Codex ${kind} keeps preview, drift, and newer-version guards`, async () => {
      const fixture = await makeTempFilesystem();
      try {
        await saveConfig(
          {
            version: 1,
            llm: { gateway: "openai", model: "gpt-5.4" },
            external: [],
            platforms: ["claude"],
          },
          fixture.fs,
          fixture.paths,
        );
        const context = { fs: fixture.fs, clock: new FixedClock(), paths: fixture.paths };
        const bundle =
          kind === "agent"
            ? BundledArtifact.agent("---\nname: helper\ndescription: Helps\n---\nBe helpful.\n")
            : BundledArtifact.skillMd("---\nname: helper\ndescription: Helps\n---\n# Helper\n");
        const newer = new ArtifactInstaller(new ArtifactIdentity("helper", "2.0.0"));
        const first = await newer.planForPlatform(bundle, "global", false, "codex", context);
        const dest = fixture.paths
          .withPlatform("codex")
          .installedArtifactPath(kind, "helper", "global");
        expect(dest).not.toBeNull();
        expect(await fixture.fs.exists(dest ?? "")).toBe(false);
        await expect(
          newer.apply(
            kind === "agent"
              ? BundledArtifact.agent("---\nname: helper\ndescription: Helps\n---\nChanged.\n")
              : BundledArtifact.skillMd("---\nname: helper\ndescription: Helps\n---\n# Changed\n"),
            first,
            context,
          ),
        ).rejects.toThrow();
        expect(await fixture.fs.exists(dest ?? "")).toBe(false);
        await newer.apply(bundle, first, context);
        const current = await newer.planForPlatform(bundle, "global", false, "codex", context);
        expect(current.plan.targets[0]?.action.kind).toBe("skip");
        const older = new ArtifactInstaller(new ArtifactIdentity("helper", "1.0.0"));
        const blocked = await older.planForPlatform(bundle, "global", false, "codex", context);
        expect(blocked.plan.targets[0]?.action.kind).toBe("refuse-newer");
        await expect(older.apply(bundle, blocked, context)).rejects.toThrow();
        const file = kind === "agent" ? (dest ?? "") : `${dest}/SKILL.md`;
        await fixture.fs.write(file, "edited locally");
        const drifted = await newer.planForPlatform(bundle, "global", false, "codex", context);
        expect(drifted.plan.targets[0]?.action.kind).toBe("drifted-skip");
        await newer.apply(bundle, drifted, context);
        expect(await fixture.fs.readText(file)).toBe("edited locally");
      } finally {
        await fixture.cleanup();
      }
    });
  }

  test("writes a Codex TOML agent and tracks it", async () => {
    const fixture = await makeTempFilesystem();
    try {
      await saveConfig(
        {
          version: 1,
          llm: { gateway: "openai", model: "gpt-5.4" },
          external: [],
          platforms: ["codex"],
        },
        fixture.fs,
        fixture.paths,
      );
      const context = { fs: fixture.fs, clock: new FixedClock(), paths: fixture.paths };
      const bundle = BundledArtifact.agent(
        "---\nname: helper\ndescription: Helps\n---\nBe helpful.\n",
      );
      const installer = new ArtifactInstaller(new ArtifactIdentity("helper", "1.0.0"));
      const plan = await installer.plan(bundle, "global", false, context);
      const result = await installer.apply(bundle, plan, context);

      expect(result.kind).toBe("agent");
      const codexPath = fixture.paths
        .withPlatform("codex")
        .installedArtifactPath("agent", "helper", "global");
      expect(codexPath).not.toBeNull();
      expect(await fixture.fs.readText(codexPath ?? "")).toContain(
        'developer_instructions = "Be helpful."',
      );
    } finally {
      await fixture.cleanup();
    }
  });
});
