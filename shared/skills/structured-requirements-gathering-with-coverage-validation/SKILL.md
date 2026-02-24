# Structured Requirements Gathering with Coverage Validation

Systematically collect and validate project requirements across multiple categories, ensuring comprehensive coverage before proceeding to downstream planning phases.

## When to Use
When you need to organize project requirements into logical categories, gather decisions/requirements for each category, validate that coverage thresholds are met, and obtain confirmation before moving to implementation planning or roadmap generation.

## Steps
1. Skip or complete any prerequisite planning phases as appropriate
2. Present the first requirement category with its name, type, and initial table stakes/decisions
3. Persist that category to save all its decisions (repeat step 2-3 for each additional category)
4. After all categories are presented and persisted, check overall coverage across all categories
5. Verify that coverage gates are met (each category has decisions, at least one v1 requirement exists)
6. Complete the requirements phase to unlock the next phase (roadmap generation, implementation, etc.)

## Tools Used
- plan_requirements (action: "present_category"): Display a requirement category with its table stakes for review
- plan_requirements (action: "persist_category"): Save all decisions/requirements for a category to the project
- plan_requirements (action: "check_coverage"): Validate that all presented categories meet coverage thresholds
- plan_requirements (action: "complete"): Finalize requirements gathering and advance to next phase
- plan_roadmap (action: "generate"): Generate downstream roadmap based on validated requirements
