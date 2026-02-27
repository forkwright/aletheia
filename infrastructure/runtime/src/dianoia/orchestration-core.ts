// Orchestration Core - Enhanced state management, rollback planning, and verification handling
// This module builds on the existing Dianoia infrastructure to provide the complete orchestration functionality

import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import { PlanningError } from "../koina/errors.js";
import { PlanningStore } from "./store.js";
import { type PlanningEvent, transition } from "./machine.js";
import { directDependents } from "./execution.js";
import type { 
  DianoiaState, 
  PlanningPhase, 
  VerificationGap, 
  VerificationResult
} from "./types.js";
import type { PhasePlan } from "./roadmap.js";

const log = createLogger("dianoia:orchestration-core");

export interface RollbackPlan {
  failedPhaseId: string;
  failureReason: string;
  skippedPhases: string[];
  rollbackActions: RollbackAction[];
  resumePoint?: string | undefined;
  timestamp: string;
}

export interface RollbackAction {
  type: "skip_phase" | "revert_change" | "manual_fix" | "checkpoint_required";
  description: string;
  phaseId?: string;
  details: Record<string, unknown>;
  priority: "high" | "medium" | "low";
}

export interface StateTransitionResult {
  success: boolean;
  fromState: DianoiaState;
  toState: DianoiaState;
  event: string;
  timestamp: string;
  metadata?: Record<string, unknown> | undefined;
}

/**
 * Enhanced orchestration core that provides:
 * 1. Clean state machine with validation
 * 2. Wave-based execution with dependency tracking
 * 3. Post-execution verification with gap detection
 * 4. Automatic rollback planning for failed verification
 * 5. Gray-area discussion question generation
 */
export class OrchestrationCore {
  private store: PlanningStore;

  constructor(db: Database.Database) {
    this.store = new PlanningStore(db);
  }

  /**
   * ORCH-01: Clean state machine with valid transitions
   * Validates and executes state transitions with comprehensive logging
   */
  executeStateTransition(
    projectId: string, 
    event: string,
    metadata?: Record<string, unknown>
  ): StateTransitionResult {
    const project = this.store.getProjectOrThrow(projectId);
    const fromState = project.state;

    try {
      // Validate transition using the state machine
      const toState = transition(fromState, event as PlanningEvent);
      
      // Execute the transition
      this.store.updateProjectState(projectId, toState);
      
      const result: StateTransitionResult = {
        success: true,
        fromState,
        toState,
        event,
        timestamp: new Date().toISOString(),
        metadata
      };

      // Emit event for monitoring
      // eventBus.emit("planning:state-transition", {
      //   projectId,
      //   ...result
      // });

      log.info(`State transition executed`, { 
        projectId, 
        fromState, 
        toState, 
        event 
      });

      return result;
    } catch (error) {
      const result: StateTransitionResult = {
        success: false,
        fromState,
        toState: fromState, // No change on failure
        event,
        timestamp: new Date().toISOString(),
        metadata: { 
          error: error instanceof Error ? error.message : String(error),
          ...metadata 
        }
      };

      log.error(`Invalid state transition rejected`, {
        projectId,
        fromState,
        event,
        error: error instanceof Error ? error.message : String(error)
      });

      return result;
    }
  }

  /**
   * ORCH-02: Wave-based execution with dependency tracking
   * The execution functionality is already implemented in execution.ts
   * This method provides a clean interface and enhanced monitoring
   */
  getExecutionStatus(projectId: string): {
    currentWave: number;
    totalWaves: number;
    completedPhases: string[];
    runningPhases: string[];
    pendingPhases: string[];
    failedPhases: string[];
    skippedPhases: string[];
  } {
    const phases = this.store.listPhases(projectId);
    const records = this.store.listSpawnRecords(projectId);

    const recordsByPhase = new Map(records.map(r => [r.phaseId, r]));
    
    const status = {
      currentWave: -1,
      totalWaves: 0,
      completedPhases: [] as string[],
      runningPhases: [] as string[],
      pendingPhases: [] as string[],
      failedPhases: [] as string[],
      skippedPhases: [] as string[]
    };

    // Calculate waves and determine current wave
    const waves = this.computeWaves(phases);
    status.totalWaves = waves.length;

    let currentWaveIndex = -1;
    for (let i = 0; i < waves.length; i++) {
      const wave = waves[i]!;
      const waveComplete = wave.every(phase => {
        const record = recordsByPhase.get(phase.id);
        return record && (record.status === 'done' || record.status === 'skipped');
      });

      if (!waveComplete) {
        currentWaveIndex = i;
        break;
      }
    }

    status.currentWave = currentWaveIndex;

    // Categorize phases by status
    for (const phase of phases) {
      const record = recordsByPhase.get(phase.id);
      
      if (!record) {
        status.pendingPhases.push(phase.id);
      } else {
        switch (record.status) {
          case 'done':
            status.completedPhases.push(phase.id);
            break;
          case 'running':
            status.runningPhases.push(phase.id);
            break;
          case 'failed':
            status.failedPhases.push(phase.id);
            break;
          case 'skipped':
            status.skippedPhases.push(phase.id);
            break;
          default:
            status.pendingPhases.push(phase.id);
        }
      }
    }

    return status;
  }

  /**
   * ORCH-03: Post-execution verification with gap detection
   * Enhanced verification reporting and gap analysis
   */
  analyzeVerificationResult(
    projectId: string, 
    phaseId: string,
    verificationResult: VerificationResult
  ): {
    overallStatus: 'passed' | 'failed' | 'needs_attention';
    criticalGaps: VerificationGap[];
    minorGaps: VerificationGap[];
    recommendations: string[];
    nextActions: string[];
  } {
    const gaps = verificationResult.gaps || [];

    // Categorize gaps by severity
    const criticalGaps = gaps.filter(gap => 
      gap.status === 'not-met' || 
      (gap.status === 'partially-met' && gap.criterion?.includes('critical'))
    );
    const minorGaps = gaps.filter(gap => 
      gap.status === 'partially-met' && !gap.criterion?.includes('critical')
    );

    // Determine overall status
    let overallStatus: 'passed' | 'failed' | 'needs_attention';
    if (verificationResult.status === 'met') {
      overallStatus = 'passed';
    } else if (criticalGaps.length > 0) {
      overallStatus = 'failed';
    } else {
      overallStatus = 'needs_attention';
    }

    // Generate recommendations
    const recommendations: string[] = [];
    const nextActions: string[] = [];

    if (criticalGaps.length > 0) {
      recommendations.push(`Address ${criticalGaps.length} critical gap(s) before proceeding`);
      nextActions.push('Fix critical issues and re-run verification');
    }

    if (minorGaps.length > 0) {
      recommendations.push(`Consider addressing ${minorGaps.length} minor gap(s) for completeness`);
      if (criticalGaps.length === 0) {
        nextActions.push('Address minor issues or override with justification');
      }
    }

    if (gaps.length === 0 && verificationResult.status === 'met') {
      recommendations.push('Phase successfully meets all success criteria');
      nextActions.push('Proceed to next phase');
    }

    log.info(`Verification analysis completed`, {
      projectId,
      phaseId,
      overallStatus,
      criticalGaps: criticalGaps.length,
      minorGaps: minorGaps.length
    });

    return {
      overallStatus,
      criticalGaps,
      minorGaps,
      recommendations,
      nextActions
    };
  }

  /**
   * ORCH-04: Automatic rollback planning for failed verification
   * Enhanced rollback plan generation with downstream impact analysis
   */
  generateRollbackPlan(
    projectId: string,
    failedPhaseId: string,
    verificationResult: VerificationResult,
    reason: string = 'Verification failed'
  ): RollbackPlan {
    const allPhases = this.store.listPhases(projectId);
    const failedPhase = allPhases.find(p => p.id === failedPhaseId);
    
    if (!failedPhase) {
      throw new PlanningError(`Phase ${failedPhaseId} not found`, { code: "PLANNING_PHASE_NOT_FOUND", context: { phaseId: failedPhaseId } });
    }

    // Find all downstream dependent phases
    const allDependentPhases = this.findAllDependentPhases(failedPhaseId, allPhases);
    
    const rollbackActions: RollbackAction[] = [];

    // Create skip actions for dependent phases
    for (const depPhase of allDependentPhases) {
      rollbackActions.push({
        type: 'skip_phase',
        description: `Skip phase "${depPhase.name}" due to dependency failure`,
        phaseId: depPhase.id,
        details: {
          reason: 'Upstream dependency failed verification',
          upstreamPhase: failedPhaseId,
          phaseName: depPhase.name
        },
        priority: 'high'
      });
    }

    // Create fix actions for verification gaps
    const gaps = verificationResult.gaps || [];
    for (const gap of gaps) {
      if (gap.proposedFix) {
        rollbackActions.push({
          type: 'manual_fix',
          description: gap.proposedFix,
          phaseId: failedPhaseId,
          details: {
            criterion: gap.criterion,
            status: gap.status,
            detail: gap.detail
          },
          priority: gap.status === 'not-met' ? 'high' : 'medium'
        });
      }
    }

    // Add checkpoint for critical decisions
    if (allDependentPhases.length > 0) {
      rollbackActions.push({
        type: 'checkpoint_required',
        description: `Manual approval required before skipping ${allDependentPhases.length} dependent phase(s)`,
        details: {
          dependentPhases: allDependentPhases.map(p => p.id),
          impact: 'High - multiple downstream phases affected'
        },
        priority: 'high'
      });
    }

    const rollbackPlan: RollbackPlan = {
      failedPhaseId,
      failureReason: reason,
      skippedPhases: allDependentPhases.map(p => p.id),
      rollbackActions,
      resumePoint: this.findResumePoint(projectId, failedPhaseId, allPhases),
      timestamp: new Date().toISOString()
    };

    // Log the rollback plan generation
    log.warn(`Rollback plan generated for phase failure`, {
      projectId,
      failedPhaseId,
      skippedPhases: rollbackPlan.skippedPhases.length,
      rollbackActions: rollbackPlan.rollbackActions.length
    });

    return rollbackPlan;
  }

  /**
   * ORCH-05: Gray-area discussion question generation
   * Enhanced question generation with context-aware options
   */
  generateDiscussionQuestions(
    projectId: string,
    phaseId: string,
    _context?: Record<string, unknown>
  ): Array<{
    question: string;
    options: Array<{ label: string; rationale: string }>;
    recommendation: string;
    priority: 'high' | 'medium' | 'low';
    category: string;
  }> {
    const phase = this.store.getPhaseOrThrow(phaseId);
    const requirements = this.store.listRequirements(projectId)
      .filter(r => phase.requirements.includes(r.reqId));

    const questions: Array<{
      question: string;
      options: Array<{ label: string; rationale: string }>;
      recommendation: string;
      priority: 'high' | 'medium' | 'low';
      category: string;
    }> = [];

    // Technology stack decisions
    const techRequirements = requirements.filter(r => 
      r.description.toLowerCase().includes('technology') ||
      r.description.toLowerCase().includes('framework') ||
      r.description.toLowerCase().includes('architecture')
    );

    if (techRequirements.length > 0) {
      questions.push({
        question: `What technology approach should we take for ${phase.name}?`,
        options: [
          {
            label: 'Modern/cutting-edge stack',
            rationale: 'Latest features and performance, but higher complexity'
          },
          {
            label: 'Proven/stable technologies',
            rationale: 'Lower risk and better team familiarity'
          },
          {
            label: 'Hybrid approach',
            rationale: 'Balance innovation with stability'
          }
        ],
        recommendation: 'Proven/stable technologies',
        priority: 'high',
        category: 'technology'
      });
    }

    // Performance vs. simplicity tradeoffs
    const performanceRequirements = requirements.filter(r =>
      r.description.toLowerCase().includes('performance') ||
      r.description.toLowerCase().includes('speed') ||
      r.description.toLowerCase().includes('scalability')
    );

    if (performanceRequirements.length > 0) {
      questions.push({
        question: `How should we handle performance optimization for ${phase.name}?`,
        options: [
          {
            label: 'Optimize early',
            rationale: 'Prevent performance debt, but adds complexity'
          },
          {
            label: 'Simple first, optimize later',
            rationale: 'Faster initial development, risk of harder optimization later'
          },
          {
            label: 'Targeted optimization',
            rationale: 'Focus only on identified bottlenecks'
          }
        ],
        recommendation: 'Targeted optimization',
        priority: 'medium',
        category: 'performance'
      });
    }

    // Testing strategy decisions
    questions.push({
      question: `What testing strategy should we implement for ${phase.name}?`,
      options: [
        {
          label: 'Comprehensive test suite',
          rationale: 'High confidence, but significant time investment'
        },
        {
          label: 'Targeted testing of critical paths',
          rationale: 'Balance coverage with development speed'
        },
        {
          label: 'Minimal testing for MVP',
          rationale: 'Fastest delivery, higher risk of bugs'
        }
      ],
      recommendation: 'Targeted testing of critical paths',
      priority: 'medium',
      category: 'quality'
    });

    // Security considerations
    const securityRequirements = requirements.filter(r =>
      r.description.toLowerCase().includes('security') ||
      r.description.toLowerCase().includes('auth') ||
      r.description.toLowerCase().includes('permission')
    );

    if (securityRequirements.length > 0) {
      questions.push({
        question: `How should we implement security for ${phase.name}?`,
        options: [
          {
            label: 'Security-first design',
            rationale: 'Maximum security, but may slow development'
          },
          {
            label: 'Standard security practices',
            rationale: 'Good balance of security and development speed'
          },
          {
            label: 'Minimal security for prototype',
            rationale: 'Fastest development, security added later'
          }
        ],
        recommendation: 'Standard security practices',
        priority: 'high',
        category: 'security'
      });
    }

    log.info(`Generated ${questions.length} discussion questions`, {
      projectId,
      phaseId,
      categories: questions.map(q => q.category)
    });

    return questions;
  }

  // Private helper methods

  private computeWaves(phases: PlanningPhase[]): PlanningPhase[][] {
    // Use existing computeWaves logic from execution.ts
    const idSet = new Set(phases.map((p) => p.id));
    const deps = new Map<string, Set<string>>();
    
    for (const phase of phases) {
      const plan = phase.plan as PhasePlan | null;
      const planDeps = (plan?.dependencies ?? []).filter((d) => idSet.has(d));
      deps.set(phase.id, new Set(planDeps));
    }

    const waves: PlanningPhase[][] = [];
    const completed = new Set<string>();
    let remaining = [...phases];

    while (remaining.length > 0) {
      const wave = remaining.filter((p) =>
        [...(deps.get(p.id) ?? new Set())].every((dep) => completed.has(dep)),
      );
      if (wave.length === 0) {
        waves.push(remaining);
        break;
      }
      waves.push(wave);
      wave.forEach((p) => completed.add(p.id));
      remaining = remaining.filter((p) => !wave.some((w) => w.id === p.id));
    }
    return waves;
  }

  private findAllDependentPhases(
    phaseId: string, 
    allPhases: PlanningPhase[]
  ): PlanningPhase[] {
    const visited = new Set<string>();
    const dependents: PlanningPhase[] = [];

    const findDependentsRecursive = (currentPhaseId: string) => {
      if (visited.has(currentPhaseId)) return;
      visited.add(currentPhaseId);

      const directDeps = directDependents(currentPhaseId, allPhases);
      for (const dep of directDeps) {
        dependents.push(dep);
        findDependentsRecursive(dep.id);
      }
    };

    findDependentsRecursive(phaseId);
    return dependents;
  }

  private findResumePoint(
    _projectId: string,
    failedPhaseId: string,
    allPhases: PlanningPhase[]
  ): string | undefined {
    // Find the earliest phase that doesn't depend on the failed phase
    const dependentPhaseIds = new Set(
      this.findAllDependentPhases(failedPhaseId, allPhases).map(p => p.id)
    );

    const independentPhases = allPhases.filter(phase => 
      phase.id !== failedPhaseId && 
      !dependentPhaseIds.has(phase.id) &&
      phase.status === 'pending'
    );

    // Return the phase with the lowest phase order
    const resumePhase = independentPhases
      .toSorted((a, b) => a.phaseOrder - b.phaseOrder)[0];

    return resumePhase?.id;
  }
}