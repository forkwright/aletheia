// Annotation types for the ground-truth conversation corpus
export interface AnnotatedConversation {
  id: string;
  agent: string;
  source: "workspace-memory" | "manual";
  messages: Array<{
    role: "user" | "assistant" | "tool_result";
    content: string;
  }>;
  expected: {
    facts: string[];
    decisions: string[];
    contradictions: string[];
    entities: string[];
  };
  metadata: {
    date: string;
    domain: string;
    notes?: string;
  };
}

export interface PerTypeMetrics {
  precision: number;
  recall: number;
  f1: number;
  matched: number;
  extracted: number;
  expected: number;
}

export interface CorpusResult {
  conversationId: string;
  agent: string;
  metrics: {
    facts: PerTypeMetrics;
    decisions: PerTypeMetrics;
    contradictions: PerTypeMetrics;
    entities: PerTypeMetrics;
    aggregate: PerTypeMetrics;
  };
  extractedFacts: string[];
  extractedDecisions: string[];
}

export interface BaselineFile {
  version: string;
  generatedAt: string;
  corpusSize: number;
  aggregate: PerTypeMetrics;
  perType: {
    facts: PerTypeMetrics;
    decisions: PerTypeMetrics;
    contradictions: PerTypeMetrics;
    entities: PerTypeMetrics;
  };
  perAgent: Record<string, PerTypeMetrics>;
}
