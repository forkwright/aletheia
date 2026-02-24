// Pure intent detection — no imports, no side effects

const ACTION_VERBS = /\b(plan|design|architect|develop|implement|start|launch)\b/i;

// "build" and "create" only match if paired with a project-scale noun
const BUILD_CREATE = /\b(build|create)\b/i;

const PROJECT_SCALE_NOUNS = /\b(project|system|app|application|service|platform|product|tool|pipeline|saas|api|backend|frontend|infrastructure|microservice)\b/i;

const COMPLEXITY_INDICATORS = /\b(roadmap|requirements|phases|architecture|milestones|scope|design doc|specifications?)\b/i;

const EXPLICIT_PHRASES = /\b(help me plan|start a project|new project|planning project|\/plan)\b/i;

export function detectPlanningIntent(text: string): boolean {
  // Explicit command-style phrases are sufficient on their own
  if (EXPLICIT_PHRASES.test(text)) return true;

  // Action verbs (plan, design, etc.) + project-scale noun → planning intent
  if (ACTION_VERBS.test(text) && PROJECT_SCALE_NOUNS.test(text)) return true;

  // "build" or "create" alone is too generic; require project-scale noun + complexity indicator
  if (BUILD_CREATE.test(text) && PROJECT_SCALE_NOUNS.test(text) && COMPLEXITY_INDICATORS.test(text)) return true;

  // Complexity indicators + project-scale noun (e.g., "requirements for building a system")
  if (COMPLEXITY_INDICATORS.test(text) && PROJECT_SCALE_NOUNS.test(text)) return true;

  return false;
}
