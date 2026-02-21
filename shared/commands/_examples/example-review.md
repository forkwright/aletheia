---
name: review
description: Review recent changes in a file
arguments:
  - name: filepath
    required: true
allowed_tools: [read, grep]
---

Review the file `$filepath` for:

- Code quality issues
- Potential bugs
- Style consistency
- Missing error handling

Provide a brief summary of findings.
