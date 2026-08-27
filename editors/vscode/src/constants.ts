export const SUPPORTED_LANGUAGES = [
  "rust",
  "python",
  "cpp",
  "c",
  "markdown",
  "yaml",
  "json",
  "jsonc",
  "toml",
  "typst",
  "java",
  "go",
  "kotlin",
  "javascript",
  "typescript",
] as const;

export const COMMANDS = {
  FORMAT_WORKSPACE: "formality.formatWorkspace",
  LINT_WORKSPACE: "formality.lintWorkspace",
  LINT_FIX: "formality.lintFix",
  SYNC: "formality.sync",
  DOCTOR: "formality.doctor",
} as const;

export const DEFAULT_EXECUTABLE = "fml";

export interface CommandDescriptor {
  args: string[];
  title: string;
  showOutput: boolean;
}

export const COMMAND_DESCRIPTORS: Record<string, CommandDescriptor> = {
  [COMMANDS.FORMAT_WORKSPACE]: {
    args: ["fmt"],
    title: "Formatting workspace...",
    showOutput: false,
  },
  [COMMANDS.LINT_WORKSPACE]: {
    args: ["lint"],
    title: "Linting workspace...",
    showOutput: true,
  },
  [COMMANDS.LINT_FIX]: {
    args: ["lint", "--fix"],
    title: "Linting workspace (auto-fix)...",
    showOutput: true,
  },
  [COMMANDS.SYNC]: {
    args: ["sync"],
    title: "Syncing native configs...",
    showOutput: false,
  },
  [COMMANDS.DOCTOR]: {
    args: ["doctor", "--all"],
    title: "Running Formality toolchain doctor...",
    showOutput: true,
  },
};
