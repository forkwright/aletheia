# Agent Interface Contracts
*Formal specifications for nous communication in the Aletheia ecosystem*

---

## Table of Contents
1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Interface Specification Format](#interface-specification-format)
4. [Capability Definitions](#capability-definitions)
5. [Message Schemas](#message-schemas)
6. [Discovery Protocol](#discovery-protocol)
7. [Contract Testing](#contract-testing)
8. [Example: Chiron Contract](#example-chiron-contract)
9. [Migration Guide](#migration-guide)

---

## Overview

The Agent Interface Contract (AIC) system provides formal specifications for agent communication, capability discovery, and contract validation in the Aletheia ecosystem. It builds upon existing infrastructure (blackboard, task contracts, status files) while adding formal interface definitions inspired by OpenAPI and ACNBP protocols.

### Key Features
- **OpenAPI-inspired** interface definitions
- **Capability-first** design with versioned contracts
- **Backward compatible** with existing blackboard/task systems
- **Self-documenting** with embedded metadata
- **Contract validation** and testing support
- **Discovery protocol** for dynamic capability negotiation

### Design Principles
1. **Explicit over Implicit** - All capabilities must be formally declared
2. **Versioned Evolution** - Contracts can evolve while maintaining compatibility
3. **Testable Interfaces** - All contracts include test specifications
4. **Self-Discovery** - Agents advertise their own capabilities
5. **Composable** - Complex workflows built from atomic capabilities

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                Agent Interface Contracts                    │
├─────────────────────────────────────────────────────────────┤
│  Discovery Service    │   Contract Registry   │  Validator  │
├─────────────────────────────────────────────────────────────┤
│           Existing Infrastructure (Enhanced)                │
├─────────────────────────────────────────────────────────────┤
│  Blackboard   │  Task Contracts  │  Status Files  │  Memory │
└─────────────────────────────────────────────────────────────┘
```

### Components

| Component | Purpose | Location |
|-----------|---------|----------|
| **Contract Registry** | Stores agent interface definitions | `/mnt/ssd/aletheia/shared/contracts/` |
| **Discovery Service** | Capability advertisement and lookup | `discover-agents` command |
| **Contract Validator** | Schema validation and testing | `validate-contract` command |
| **Capability Matcher** | Find agents for specific tasks | `match-capability` command |

---

## Interface Specification Format

Agent contracts use YAML format inspired by OpenAPI 3.1 with agent-specific extensions.

### Core Structure

```yaml
# /mnt/ssd/aletheia/shared/contracts/{agent-id}.yaml
contractVersion: "1.0"
agent:
  id: "chiron"
  name: "Work Domain Agent"
  version: "2.1.0"
  description: "Specialized in business analysis, SQL queries, and dashboard creation"
  
info:
  title: "Chiron Agent Interface"
  description: "Professional work domain agent for Summus Global"
  contact:
    session: "agent:chiron:main"
  license:
    name: "Internal Use"
    
servers:
  - description: "Primary session endpoint"
    url: "session://agent:chiron:main"
    protocol: "aletheia-session"

capabilities:
  # Capability definitions...
  
schemas:
  # Message type definitions...
  
tests:
  # Contract test specifications...
```

### Capability Definition Schema

```yaml
capabilities:
  capability_name:
    summary: "Brief description"
    description: "Detailed explanation of what this capability does"
    domain: "work|home|school|craft|meta"
    method: "query|analysis|synthesis|execution|monitoring"
    
    input:
      type: "object"
      required: ["field1", "field2"]
      properties:
        field1:
          type: "string"
          description: "Input parameter description"
        field2:
          type: "number"
          minimum: 0
          
    output:
      type: "object"
      required: ["result"]
      properties:
        result:
          type: "string"
          description: "Output format description"
          
    sla:
      maxDuration: "PT30M"          # ISO 8601 duration
      progressInterval: "PT5M"      # Progress update frequency
      reliability: 0.95             # Success rate expectation
      
    dependencies:
      - capability: "data_access"   # Other capabilities needed
        version: ">=1.0"
      - external: "summus_api"      # External service dependencies
        
    examples:
      - name: "Basic SQL Query"
        input:
          query: "SELECT * FROM sales WHERE quarter = 'Q4'"
          format: "table"
        output:
          result: "Formatted query results"
          artifacts: ["sql_output.csv"]
```

---

## Capability Definitions

### Standard Capability Categories

| Category | Purpose | Examples |
|----------|---------|----------|
| **query** | Information retrieval | Search data, fetch status, lookup facts |
| **analysis** | Data processing & insights | Trend analysis, performance review |
| **synthesis** | Combining information | Report generation, summary creation |
| **execution** | Direct actions | Create events, send emails, run scripts |
| **monitoring** | Ongoing observation | Track metrics, watch for changes |
| **coordination** | Cross-agent orchestration | Delegate tasks, resolve conflicts |

### Capability Metadata

Each capability includes:
- **Semantic tags** for discovery (`["sql", "business", "reporting"]`)
- **Resource requirements** (memory, time, external APIs)
- **Quality metrics** (accuracy, reliability, performance)
- **Versioning** with backwards compatibility rules

---

## Message Schemas

### Enhanced Task Contract Schema

Extends the existing task contract schema with capability-specific fields:

```yaml
schemas:
  CapabilityRequest:
    type: "object"
    required: ["capability", "version", "input"]
    properties:
      capability:
        type: "string"
        description: "Target capability name"
      version:
        type: "string"
        pattern: "^\\d+\\.\\d+(\\.\\d+)?$"
        description: "Required capability version (semver)"
      input:
        type: "object"
        description: "Input parameters matching capability schema"
      metadata:
        type: "object"
        properties:
          timeout: 
            type: "string"
            description: "ISO 8601 duration override"
          priority:
            type: "string"
            enum: ["low", "medium", "high", "urgent"]
          
  CapabilityResponse:
    type: "object" 
    required: ["success", "output"]
    properties:
      success:
        type: "boolean"
      output:
        type: "object"
        description: "Response matching capability output schema"
      error:
        type: "object"
        properties:
          code: 
            type: "string"
            enum: ["invalid_input", "capability_unavailable", "timeout", "system_error"]
          message:
            type: "string"
          details:
            type: "object"
      metadata:
        type: "object"
        properties:
          executionTime:
            type: "string"
            description: "ISO 8601 duration"
          resourcesUsed:
            type: "array"
            items:
              type: "string"
```

### Discovery Messages

```yaml
schemas:
  CapabilityAdvertisement:
    type: "object"
    required: ["agent_id", "capabilities", "timestamp"]
    properties:
      agent_id:
        type: "string"
        enum: ["syn", "syl", "chiron", "eiron", "demiurge"]
      capabilities:
        type: "array"
        items:
          type: "object"
          properties:
            name: 
              type: "string"
            version:
              type: "string" 
            available:
              type: "boolean"
            load:
              type: "number"
              description: "Current load factor (0.0-1.0)"
      timestamp:
        type: "string"
        format: "date-time"
        
  CapabilityQuery:
    type: "object"
    required: ["requirements"]
    properties:
      requirements:
        type: "object"
        properties:
          domain:
            type: "string"
            enum: ["work", "home", "school", "craft", "meta"]
          method:
            type: "string"
            enum: ["query", "analysis", "synthesis", "execution", "monitoring"]
          tags:
            type: "array"
            items:
              type: "string"
          maxDuration:
            type: "string"
            description: "ISO 8601 duration"
```

---

## Discovery Protocol

### Capability Advertisement

Agents periodically advertise their capabilities:

```bash
# Agents run during heartbeat or startup
advertise-capabilities --agent chiron --contract /mnt/ssd/aletheia/shared/contracts/chiron.yaml
```

### Capability Discovery

Find agents with specific capabilities:

```bash
# Interactive discovery
discover-agents --domain work --method analysis --tags "sql,reporting"

# Programmatic lookup  
match-capability --input '{"domain":"work","tags":["sql"],"maxDuration":"PT30M"}'
```

### Discovery Flow

1. **Registration**: Agent registers contract with registry on startup
2. **Advertisement**: Agent periodically broadcasts availability 
3. **Query**: Requesting agent queries for needed capabilities
4. **Matching**: Discovery service returns ranked list of capable agents
5. **Negotiation**: Requesting agent can validate compatibility before tasking

### Registry Structure

```
/mnt/ssd/aletheia/shared/contracts/
├── registry.json           # Active agent registry
├── chiron.yaml            # Chiron's contract
├── syl.yaml               # Syl's contract  
├── eiron.yaml             # Eiron's contract
├── demiurge.yaml          # Demiurge's contract
├── syn.yaml               # Syn's contract (meta-capabilities)
└── schemas/
    ├── capability.schema.json
    └── discovery.schema.json
```

---

## Contract Testing

### Test Specification Format

Each capability includes test cases:

```yaml
tests:
  sql_query:
    - name: "Basic SELECT query"
      description: "Test simple SQL SELECT execution"
      input:
        query: "SELECT COUNT(*) FROM test_table"
        format: "scalar"
      expected_output:
        result: "42"
        type: "number"
      sla_requirements:
        maxDuration: "PT30S"
        
    - name: "Invalid query handling"
      description: "Test error handling for malformed SQL"
      input:
        query: "INVALID SQL SYNTAX"
        format: "table"
      expected_error:
        code: "invalid_input"
        message: "SQL syntax error"
```

### Validation Commands

```bash
# Validate contract schema
validate-contract /mnt/ssd/aletheia/shared/contracts/chiron.yaml

# Run capability tests
test-capability --agent chiron --capability sql_query --verbose

# Test inter-agent communication
test-integration --source syn --target chiron --capability sql_query

# Validate all contracts
validate-all-contracts --fix-errors
```

### Continuous Validation

Tests run automatically:
- **On contract changes** (validate before deployment)
- **During agent startup** (self-test critical capabilities)  
- **Periodic health checks** (detect capability degradation)
- **Before major delegations** (ensure compatibility)

---

## Example: Chiron Contract

```yaml
contractVersion: "1.0"
agent:
  id: "chiron" 
  name: "Work Domain Agent"
  version: "2.1.0"
  description: "Professional work agent specialized in business analysis and SQL operations for Summus Global"

info:
  title: "Chiron Agent Interface Contract"
  description: |
    Chiron handles all work-related tasks including SQL queries, dashboard creation, 
    ROI calculations, and business analysis. Primary interface to Summus Global systems.
  contact:
    session: "agent:chiron:main"
    documentation: "/mnt/ssd/aletheia/nous/arbor/"
  license:
    name: "Internal Use"
    
servers:
  - description: "Primary session endpoint"
    url: "session://agent:chiron:main" 
    protocol: "aletheia-session"
  - description: "Claude Code interface"
    url: "tmux://work"
    protocol: "claude-code"

capabilities:
  sql_query:
    summary: "Execute SQL queries against business databases"
    description: |
      Execute read-only SQL queries against Summus Global databases. 
      Supports SELECT statements with joins, aggregations, and window functions.
      Returns formatted results as tables, CSV, or scalar values.
    domain: "work"
    method: "query"
    tags: ["sql", "database", "business-data"]
    
    input:
      type: "object"
      required: ["query"]
      properties:
        query:
          type: "string"
          description: "SQL SELECT query to execute"
          maxLength: 10000
          pattern: "^\\s*SELECT\\s+"  # Only SELECT allowed
        database:
          type: "string"
          enum: ["core", "sms", "rso", "compliance"]
          default: "core"
          description: "Target database schema"
        format:
          type: "string" 
          enum: ["table", "csv", "json", "scalar"]
          default: "table"
          description: "Output format preference"
        limit:
          type: "integer"
          minimum: 1
          maximum: 10000
          default: 1000
          description: "Maximum rows to return"
          
    output:
      type: "object"
      required: ["result", "rowCount"]
      properties:
        result:
          type: "string"
          description: "Formatted query results"
        rowCount:
          type: "integer"
          description: "Number of rows returned"
        executionTime:
          type: "string"
          description: "Query execution duration (ISO 8601)"
        artifacts:
          type: "array"
          items:
            type: "string"
          description: "Paths to generated files (CSV exports, etc.)"
          
    sla:
      maxDuration: "PT5M"
      progressInterval: "PT30S" 
      reliability: 0.98
      
    dependencies:
      - external: "metis_ssh"
        description: "SSH access to Metis for work Claude Code"
      - external: "summus_db"
        description: "Database access via work environment"
        
    examples:
      - name: "Customer count by region"
        input:
          query: "SELECT region, COUNT(*) as customers FROM accounts GROUP BY region"
          format: "table"
        output:
          result: |
            | Region    | Customers |
            |-----------|-----------|
            | North     | 1,234     |
            | South     | 987       |
            | East      | 2,100     |
            | West      | 1,456     |
          rowCount: 4
          executionTime: "PT1.2S"

  dashboard_analysis:
    summary: "Analyze dashboard performance and metrics"
    description: |
      Review existing dashboards for data accuracy, performance issues, 
      and optimization opportunities. Generate improvement recommendations.
    domain: "work"
    method: "analysis"
    tags: ["dashboard", "performance", "analytics", "reporting"]
    
    input:
      type: "object"
      required: ["dashboard_name"]
      properties:
        dashboard_name:
          type: "string"
          description: "Name or path of dashboard to analyze"
        analysis_type:
          type: "string"
          enum: ["performance", "accuracy", "usage", "comprehensive"]
          default: "comprehensive"
        time_range:
          type: "string"
          description: "Analysis time window (ISO 8601 duration)"
          default: "P30D"  # Last 30 days
          
    output:
      type: "object"
      required: ["summary", "recommendations"]
      properties:
        summary:
          type: "object"
          properties:
            performance_score:
              type: "number"
              minimum: 0
              maximum: 100
            data_accuracy:
              type: "number"
              minimum: 0
              maximum: 100
            usage_frequency:
              type: "string"
              enum: ["high", "medium", "low"]
        recommendations:
          type: "array"
          items:
            type: "object"
            properties:
              priority:
                type: "string"
                enum: ["high", "medium", "low"]
              category:
                type: "string"
                enum: ["performance", "data", "ui", "access"]
              description:
                type: "string"
              estimated_impact:
                type: "string"
        artifacts:
          type: "array"
          items:
            type: "string"
          description: "Generated reports, screenshots, etc."
          
    sla:
      maxDuration: "PT30M"
      progressInterval: "PT5M"
      reliability: 0.95

  roi_calculation:
    summary: "Calculate return on investment for business initiatives"
    description: |
      Perform ROI analysis using standardized business formulas.
      Supports multiple scenarios and sensitivity analysis.
    domain: "work"  
    method: "analysis"
    tags: ["finance", "roi", "business-case", "modeling"]
    
    input:
      type: "object"
      required: ["investment", "returns"]
      properties:
        investment:
          type: "object"
          required: ["initial_cost"]
          properties:
            initial_cost:
              type: "number"
              description: "Upfront investment amount"
            ongoing_costs:
              type: "number"
              description: "Annual ongoing costs"
              default: 0
        returns:
          type: "object"
          required: ["annual_benefit"]
          properties:
            annual_benefit:
              type: "number"
              description: "Expected annual benefit/savings"
            time_horizon:
              type: "integer"
              description: "Analysis period in years"
              default: 5
            discount_rate:
              type: "number"
              description: "Discount rate for NPV calculation"
              default: 0.08
        scenarios:
          type: "array"
          items:
            type: "object"
            properties:
              name:
                type: "string"
              benefit_multiplier:
                type: "number"
                description: "Factor to multiply base annual benefit"
          description: "Optional scenario analysis"
          
    output:
      type: "object"
      required: ["roi_percentage", "payback_period", "npv"]
      properties:
        roi_percentage:
          type: "number"
          description: "Return on investment as percentage"
        payback_period:
          type: "number"
          description: "Payback period in years"
        npv:
          type: "number"
          description: "Net present value"
        irr:
          type: "number"
          description: "Internal rate of return"
        scenario_results:
          type: "array"
          items:
            type: "object"
          description: "Results for each scenario"
        artifacts:
          type: "array"
          items:
            type: "string"
          description: "Detailed analysis files"
          
    sla:
      maxDuration: "PT10M"
      progressInterval: "PT2M"
      reliability: 0.99

  project_status:
    summary: "Monitor and report on work project progress"
    description: |
      Track project milestones, identify blockers, and generate status reports.
      Integrates with Taskwarrior and project documentation.
    domain: "work"
    method: "monitoring"
    tags: ["project-management", "status", "tracking", "reporting"]
    
    input:
      type: "object"
      required: ["project_name"]
      properties:
        project_name:
          type: "string"
          description: "Project identifier or name"
        report_type:
          type: "string"
          enum: ["brief", "detailed", "executive"]
          default: "brief"
        include_blockers:
          type: "boolean"
          default: true
          
    output:
      type: "object"
      required: ["status", "completion_percentage"]
      properties:
        status:
          type: "string"
          enum: ["on_track", "at_risk", "blocked", "completed"]
        completion_percentage:
          type: "number"
          minimum: 0
          maximum: 100
        blockers:
          type: "array"
          items:
            type: "object"
            properties:
              description:
                type: "string"
              priority:
                type: "string"
                enum: ["high", "medium", "low"]
              owner:
                type: "string"
        next_milestones:
          type: "array"
          items:
            type: "object"
            properties:
              name:
                type: "string"
              due_date:
                type: "string"
                format: "date"
              at_risk:
                type: "boolean"
        artifacts:
          type: "array"
          items:
            type: "string"
          description: "Generated reports"
          
    sla:
      maxDuration: "PT15M"
      progressInterval: "PT3M"
      reliability: 0.97

schemas:
  WorkTaskContext:
    type: "object"
    properties:
      client:
        type: "string"
        description: "Client or project context"
      urgency:
        type: "string"
        enum: ["routine", "priority", "urgent", "emergency"]
      confidentiality:
        type: "string"
        enum: ["public", "internal", "confidential", "restricted"]
      stakeholders:
        type: "array"
        items:
          type: "string"
        description: "People who should be notified of results"

tests:
  sql_query:
    - name: "Basic table query"
      description: "Test simple SELECT with formatting"
      input:
        query: "SELECT 'test' as column1, 42 as column2"
        format: "table"
      expected_output:
        result: "| column1 | column2 |\n|---------|--------|\n| test    | 42      |"
        rowCount: 1
      sla_requirements:
        maxDuration: "PT30S"
        
    - name: "Invalid SQL handling"
      description: "Test error handling for malformed queries"
      input:
        query: "INVALID SQL SYNTAX HERE"
        format: "table"
      expected_error:
        code: "invalid_input"
      sla_requirements:
        maxDuration: "PT10S"

  roi_calculation:
    - name: "Simple ROI calculation"
      description: "Test basic ROI calculation with known values"
      input:
        investment:
          initial_cost: 100000
        returns:
          annual_benefit: 25000
          time_horizon: 5
      expected_output:
        roi_percentage: 25.0
        payback_period: 4.0
      sla_requirements:
        maxDuration: "PT1M"

metadata:
  version: "2.1.0"
  last_updated: "2026-02-03T20:30:00Z"
  contract_maintainer: "syn"
  supported_protocols: ["aletheia-session", "claude-code", "blackboard", "task-contract"]
  reliability_sla: 0.95
  
  # Integration mappings
  blackboard_mappings:
    sql_query: "work"
    dashboard_analysis: "work"
    roi_calculation: "work" 
    project_status: "work"
    
  task_contract_mappings:
    sql_query: "query"
    dashboard_analysis: "analysis"
    roi_calculation: "analysis"
    project_status: "monitoring"
```

---

## Migration Guide

### Phase 1: Contract Creation (Week 1)
1. **Create initial contracts** for all 5 agents using existing status files
2. **Deploy contract registry** and discovery commands
3. **Update agent startup** to register capabilities  
4. **Backward compatibility** - existing systems continue unchanged

### Phase 2: Enhanced Communication (Week 2) 
1. **Upgrade task contracts** to include capability versions
2. **Implement validation** in task creation and delegation
3. **Add capability matching** to automatic routing  
4. **Test integration** between old and new systems

### Phase 3: Full Adoption (Week 3-4)
1. **Migrate blackboard tasks** to use capability-based routing
2. **Implement contract testing** in CI/CD pipeline
3. **Add monitoring** for capability performance and reliability
4. **Documentation update** for all agent maintainers

### Backward Compatibility

The contract system maintains full backward compatibility:

- **Existing blackboard tasks** continue to work unchanged
- **Current task contracts** remain valid (enhanced with optional fields)
- **Agent status files** continue to be used (enhanced with capability info)
- **Manual delegation** via `sessions_send` still supported

### Implementation Commands

```bash
# Phase 1 setup
bootstrap-contracts --create-all --from-status
deploy-contract-registry
register-agent-capabilities

# Phase 2 enhancement  
upgrade-task-contracts --validate-capabilities
enable-capability-routing --gradual-rollout

# Phase 3 completion
migrate-blackboard-to-capabilities
enable-contract-testing --all-agents
monitor-capability-performance
```

---

## Implementation Notes

### File Locations

```
/mnt/ssd/aletheia/shared/contracts/
├── registry.json              # Active capability registry
├── {agent}.yaml              # Agent interface contracts
└── schemas/                  # JSON schemas for validation

/mnt/ssd/aletheia/shared/bin/
├── advertise-capabilities    # Capability advertisement
├── discover-agents           # Agent discovery
├── match-capability          # Capability matching
├── validate-contract         # Contract validation
├── test-capability           # Capability testing
└── bootstrap-contracts       # Migration tooling
```

### Integration Points

| System | Integration |
|--------|-------------|
| **Blackboard** | Enhanced with capability routing |
| **Task Contracts** | Capability version requirements |
| **Agent Status** | Capability health monitoring |
| **Memory Memory** | Capability usage history |
| **Facts.jsonl** | Capability performance metrics |

### Security Considerations

1. **Input Validation** - All capability inputs validated against schema
2. **Capability Verification** - Agents must prove they support claimed capabilities
3. **Resource Limits** - SLA enforcement prevents resource exhaustion
4. **Access Control** - Capability access can be restricted by domain/agent
5. **Audit Trail** - All capability usage logged for security analysis

---

*Agent Interface Contract System v1.0*  
*Created: 2026-02-03*  
*Maintainer: Syn (Orchestrator Agent)*  
*Status: Design Complete - Ready for Implementation*