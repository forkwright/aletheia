---
name: deploy
description: Deploy a service from a branch
arguments:
  - name: service
    required: true
  - name: branch
    default: main
allowed_tools: [exec, read]
---

Deploy `$service` from branch `$branch`.

1. Verify all tests pass on `$branch`
2. Build the service
3. Deploy to production
4. Run health checks
5. Report status
