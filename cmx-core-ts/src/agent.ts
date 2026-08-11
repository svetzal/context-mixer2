const splitMarkdown = (content: string): { frontmatter?: string; body: string } => {
  const open = content.startsWith("---\n")
    ? "---\n"
    : content.startsWith("---\r\n")
      ? "---\r\n"
      : undefined;
  if (open === undefined) return { body: content };
  const rest = content.slice(open.length);
  let lineStart = 0;
  while (lineStart <= rest.length) {
    const lineEnd = rest.indexOf("\n", lineStart);
    const hasNewline = lineEnd !== -1;
    const rawLine = rest.slice(lineStart, hasNewline ? lineEnd : rest.length);
    if (rawLine.replace(/\r$/u, "") === "---") {
      return {
        frontmatter: rest.slice(0, lineStart),
        body: hasNewline ? rest.slice(lineEnd + 1) : "",
      };
    }
    if (!hasNewline) break;
    lineStart = lineEnd + 1;
  }
  return { body: content };
};

const field = (frontmatter: string | undefined, key: string): string | undefined => {
  if (frontmatter === undefined) return undefined;
  const prefix = `${key}:`;
  for (const line of frontmatter.split(/\r?\n/u)) {
    if (line.startsWith(" ") || line.startsWith("\t") || !line.startsWith(prefix)) continue;
    return line
      .slice(prefix.length)
      .trim()
      .replace(/^['"]+|['"]+$/gu, "");
  }
  return undefined;
};

const tomlString = (value: string): string => {
  let output = '"';
  for (const character of value) {
    switch (character) {
      case '"':
        output += '\\"';
        break;
      case "\\":
        output += "\\\\";
        break;
      case "\b":
        output += "\\b";
        break;
      case "\t":
        output += "\\t";
        break;
      case "\n":
        output += "\\n";
        break;
      case "\f":
        output += "\\f";
        break;
      case "\r":
        output += "\\r";
        break;
      default: {
        const codePoint = character.codePointAt(0) ?? 0;
        output +=
          codePoint < 0x20 || codePoint === 0x7f
            ? `\\u${codePoint.toString(16).toUpperCase().padStart(4, "0")}`
            : character;
      }
    }
  }
  return `${output}"`;
};

/** Convert a markdown agent into Codex's TOML subagent format. */
export const markdownToCodexToml = (markdown: string, fallbackName: string): string => {
  const { frontmatter, body } = splitMarkdown(markdown);
  const name = field(frontmatter, "name") || fallbackName;
  const description = field(frontmatter, "description") ?? "";
  const model = field(frontmatter, "model");
  const lines = [`name = ${tomlString(name)}`, `description = ${tomlString(description)}`];
  if (model !== undefined && model.length > 0) lines.push(`model = ${tomlString(model)}`);
  lines.push(`developer_instructions = ${tomlString(body.replace(/\n+$/u, ""))}`);
  return `${lines.join("\n")}\n`;
};
