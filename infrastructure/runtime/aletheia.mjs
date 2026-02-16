#!/usr/bin/env node
// Aletheia runtime bootstrap
import module from "node:module";

if (module.enableCompileCache && !process.env.NODE_DISABLE_COMPILE_CACHE) {
  try {
    module.enableCompileCache();
  } catch {
    // ignore
  }
}

await import("./dist/entry.mjs");
