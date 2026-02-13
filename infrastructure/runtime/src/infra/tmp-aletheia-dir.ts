import fs from "node:fs";
import os from "node:os";
import path from "node:path";

export const POSIX_ALETHEIA_TMP_DIR = "/tmp/aletheia";

type ResolvePreferredAletheiaTmpDirOptions = {
  accessSync?: (path: string, mode?: number) => void;
  statSync?: (path: string) => { isDirectory(): boolean };
  tmpdir?: () => string;
};

type MaybeNodeError = { code?: string };

function isNodeErrorWithCode(err: unknown, code: string): err is MaybeNodeError {
  return (
    typeof err === "object" &&
    err !== null &&
    "code" in err &&
    (err as MaybeNodeError).code === code
  );
}

export function resolvePreferredAletheiaTmpDir(
  options: ResolvePreferredAletheiaTmpDirOptions = {},
): string {
  const accessSync = options.accessSync ?? fs.accessSync;
  const statSync = options.statSync ?? fs.statSync;
  const tmpdir = options.tmpdir ?? os.tmpdir;

  try {
    const preferred = statSync(POSIX_ALETHEIA_TMP_DIR);
    if (!preferred.isDirectory()) {
      return path.join(tmpdir(), "aletheia");
    }
    accessSync(POSIX_ALETHEIA_TMP_DIR, fs.constants.W_OK | fs.constants.X_OK);
    return POSIX_ALETHEIA_TMP_DIR;
  } catch (err) {
    if (!isNodeErrorWithCode(err, "ENOENT")) {
      return path.join(tmpdir(), "aletheia");
    }
  }

  try {
    accessSync("/tmp", fs.constants.W_OK | fs.constants.X_OK);
    return POSIX_ALETHEIA_TMP_DIR;
  } catch {
    return path.join(tmpdir(), "aletheia");
  }
}
