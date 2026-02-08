type StateDirEnvSnapshot = {
  aletheiaStateDir: string | undefined;
};

export function snapshotStateDirEnv(): StateDirEnvSnapshot {
  return {
    aletheiaStateDir: process.env.ALETHEIA_STATE_DIR,
  };
}

export function restoreStateDirEnv(snapshot: StateDirEnvSnapshot): void {
  if (snapshot.aletheiaStateDir === undefined) {
    delete process.env.ALETHEIA_STATE_DIR;
  } else {
    process.env.ALETHEIA_STATE_DIR = snapshot.aletheiaStateDir;
  }
}

export function setStateDirEnv(stateDir: string): void {
  process.env.ALETHEIA_STATE_DIR = stateDir;
}
