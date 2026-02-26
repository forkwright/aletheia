// ESLint flat config for Svelte 5 template-layer analysis
import eslintPluginSvelte from "eslint-plugin-svelte";
import globals from "globals";
import tsParser from "@typescript-eslint/parser";

export default [
  ...eslintPluginSvelte.configs["flat/recommended"],
  {
    files: ["**/*.svelte"],
    languageOptions: {
      globals: { ...globals.browser },
      parserOptions: {
        parser: tsParser,
      },
    },
    rules: {
      "svelte/no-at-html-tags": "error",
      "svelte/no-target-blank": "error",
      // Allow $state() wrapping when the variable is reassigned (needed for reactivity)
      "svelte/no-unnecessary-state-wrap": ["error", { "allowReassign": true }],
      // Deferred to Phase 13 remediation — widespread in existing codebase
      "svelte/require-each-key": "warn",
      "svelte/prefer-svelte-reactivity": "warn",
    },
  },
  {
    // svelte-eslint-parser is wired for .svelte.ts files in flat/recommended
    // requires TypeScript sub-parser to avoid parse errors
    files: ["**/*.svelte.ts", "**/*.svelte.js"],
    languageOptions: {
      parserOptions: {
        parser: tsParser,
      },
    },
    rules: {
      // Allow $state() wrapping when the variable is reassigned (needed for reactivity)
      "svelte/no-unnecessary-state-wrap": ["error", { "allowReassign": true }],
      // Deferred to Phase 13 remediation — widespread in existing store files
      "svelte/prefer-svelte-reactivity": "warn",
    },
  },
  {
    ignores: ["dist/", "node_modules/"],
  },
];
