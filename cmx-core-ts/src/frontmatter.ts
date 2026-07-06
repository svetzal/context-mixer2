import type { SkillFile } from "./skill-fs.ts";

const textDecoder = new TextDecoder();
const textEncoder = new TextEncoder();

export const reconcileSkillVersion = (files: readonly SkillFile[], version: string): SkillFile[] =>
  files.map((file) => {
    if (file.relPath !== "SKILL.md") {
      return { ...file, bytes: new Uint8Array(file.bytes) };
    }

    const content = textDecoder.decode(file.bytes);
    const reconciled = setMetadataVersion(content, version);
    return {
      relPath: file.relPath,
      bytes: textEncoder.encode(reconciled),
    };
  });

export const setMetadataVersion = (content: string, version: string): string => {
  const value = `"${version}"`;
  const open = content.startsWith("---\n")
    ? "---\n"
    : content.startsWith("---\r\n")
      ? "---\r\n"
      : undefined;
  if (open === undefined) {
    return content;
  }

  const afterOpen = content.slice(open.length);
  const fenceStart = findClosingFence(afterOpen);
  if (fenceStart === undefined) {
    return content;
  }

  const inner = afterOpen.slice(0, fenceStart);
  const closingAndRest = afterOpen.slice(fenceStart);
  return `${open}${reconcileInner(inner, value)}${closingAndRest}`;
};

const findClosingFence = (afterOpen: string): number | undefined => {
  let lineStart = 0;

  while (lineStart <= afterOpen.length) {
    const rest = afterOpen.slice(lineStart);
    const newlineIndex = rest.indexOf("\n");
    const hasNewline = newlineIndex !== -1;
    const line = hasNewline ? rest.slice(0, newlineIndex) : rest;

    if (line.replace(/\r$/u, "") === "---") {
      return lineStart;
    }

    if (!hasNewline) {
      return undefined;
    }

    lineStart += line.length + 1;
  }

  return undefined;
};

const reconcileInner = (inner: string, value: string): string => {
  const lines = inner.match(/[^\n]*\n|[^\n]+/gu) ?? [];
  const withoutTopLevelVersion = lines.filter((line) => !isTopLevelKey(line, "version"));
  const metadataIndex = withoutTopLevelVersion.findIndex((line) => isTopLevelKey(line, "metadata"));

  if (metadataIndex === -1) {
    ensureTrailingNewline(withoutTopLevelVersion);
    withoutTopLevelVersion.push("metadata:\n");
    withoutTopLevelVersion.push(`  version: ${value}\n`);
    return withoutTopLevelVersion.join("");
  }

  setVersionInMetadataBlock(withoutTopLevelVersion, metadataIndex, value);
  return withoutTopLevelVersion.join("");
};

const setVersionInMetadataBlock = (lines: string[], metadataIndex: number, value: string): void => {
  let firstChildIndent: string | undefined;
  let versionIndex: number | undefined;

  for (let index = metadataIndex + 1; index < lines.length; index += 1) {
    const line = lines[index] ?? "";
    const trimmed = line.replace(/[\n\r]+$/u, "");
    if (trimmed.trim().length === 0) {
      continue;
    }

    const indent = leadingWhitespace(trimmed);
    if (indent.length === 0) {
      break;
    }

    firstChildIndent ??= indent;
    if (trimmed.trimStart().startsWith("version:")) {
      versionIndex = index;
      break;
    }
  }

  if (versionIndex !== undefined) {
    const indent = leadingWhitespace(lines[versionIndex] ?? "");
    lines[versionIndex] = `${indent}version: ${value}\n`;
    return;
  }

  lines.splice(metadataIndex + 1, 0, `${firstChildIndent ?? "  "}version: ${value}\n`);
};

const isTopLevelKey = (line: string, key: string): boolean => {
  const trimmed = line.replace(/[\n\r]+$/u, "");
  return (
    !line.startsWith(" ") &&
    !line.startsWith("\t") &&
    trimmed.startsWith(key) &&
    trimmed.slice(key.length).startsWith(":")
  );
};

const leadingWhitespace = (value: string): string => value.match(/^[ \t]*/u)?.[0] ?? "";

const ensureTrailingNewline = (lines: string[]): void => {
  if (lines.length === 0) {
    return;
  }

  const lastIndex = lines.length - 1;
  if (!lines[lastIndex]?.endsWith("\n")) {
    lines[lastIndex] = `${lines[lastIndex] ?? ""}\n`;
  }
};
