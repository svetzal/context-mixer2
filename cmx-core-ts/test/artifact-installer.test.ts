import { describe, expect, test } from "bun:test";

import { ArtifactIdentity, ArtifactInstaller, BundledArtifact } from "../src/artifact-installer.ts";
import { saveConfig } from "../src/config.ts";
import { FixedClock, makeTempFilesystem } from "./helpers.ts";

describe("ArtifactInstaller", () => {
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
