# Production Deployment Hardening with Docker & Git Workflow

Implement and validate production-grade security, logging, and deployment configurations for a containerized application, then commit and merge changes through a feature branch.

## When to Use

This pattern is applicable when:
- Hardening an existing containerized application for production deployment
- Adding reverse proxy, security policies, and logging infrastructure
- Coordinating multiple configuration files (Docker, networking, secrets)
- Building, testing, and validating changes before merging to main
- Working with Git feature branches and pull requests for code review

## Steps

1. **Audit current deployment** — Read relevant specification documents and inspect running containers to understand current state (resource limits, restart policies, logging, security settings)
2. **Create feature branch** — Initialize a Git feature branch for the production hardening work
3. **Inventory infrastructure** — Query Docker containers to identify gaps (reverse proxy, auto-update mechanisms, resource constraints)
4. **Add reverse proxy configuration** — Write Caddy/Nginx configuration files for both production (HTTPS/Let's Encrypt) and local network deployments
5. **Create deployment overlays** — Write Docker Compose overlay files for secrets management, reverse proxy integration, and backup automation
6. **Enhance application security** — Edit source code to upgrade logging (Serilog), add security middleware, and implement proper secret handling
7. **Update container image** — Modify Dockerfile to add security hardening (non-root user, read-only filesystem, resource limits, security headers)
8. **Create deployment documentation** — Write README with quick-start and troubleshooting guidance
9. **Commit infrastructure changes** — Stage and commit all configuration and deployment scripts with descriptive messages
10. **Remote build and test** — SSH to remote builder, fetch feature branch, build Docker image, and run integration tests
11. **Local validation** — Build image locally, run container with test configuration, verify logging output and health endpoints
12. **Fix discovered issues** — Edit and commit additional fixes discovered during testing (e.g., logger initialization order)
13. **Create pull request** — Submit feature branch to GitHub with comprehensive description of changes
14. **Merge and deploy** — Squash-merge PR to main, pull latest, and proceed with production deployment

## Tools Used

- exec: Run shell commands for Docker inspection, Git operations, builds, and remote SSH sessions
- read: Inspect existing configuration files (docker-compose.yml, source code) to understand current state
- write: Create new deployment configuration files (Caddy, docker-compose overlays, deployment scripts, README)
- edit: Modify source code and configuration files in-place (Dockerfile, Program.cs, SerilogConfiguration.cs, .csproj)
