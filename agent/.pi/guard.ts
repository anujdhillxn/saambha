// pi extension (loaded with `pi -e`): blocks sudo and rm -rf in the bash tool.
// Claude Code gets the same rules from .claude/settings.json.
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const RULES: [RegExp, string][] = [
  [/(^|&&|;|\||\s)sudo(\s|$)/, "sudo is not allowed"],
  [/rm\s+-[a-zA-Z]*r[a-zA-Z]*f|rm\s+-[a-zA-Z]*f[a-zA-Z]*r/, "rm -rf is not allowed"],
];

export default function (pi: ExtensionAPI) {
  pi.on("tool_call", async (event) => {
    if (event.toolName !== "bash") return;
    const command = (event.input as { command?: string }).command ?? "";
    for (const [pattern, reason] of RULES) {
      if (pattern.test(command)) return { block: true, reason };
    }
  });
}
