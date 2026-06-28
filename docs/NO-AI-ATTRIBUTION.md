# No AI Attribution Policy

Aletheia keeps generated-output attribution markers out of public PR metadata
and commit history. The policy covers PR titles, PR bodies, and commit messages
because all three can become durable public release history.

The checked source of truth is
`.github/no-ai-attribution-patterns.txt`. The workflow
`.github/workflows/no-ai-attribution.yml` reads that file directly for both PR
metadata and commit-message scans, so the two checks cannot drift.

The pattern file is a POSIX extended regular expression list consumed with
case-insensitive `grep -E -f`. To update the policy, edit that file and keep the
workflow unchanged unless the scan surfaces or enforcement behavior also need to
change.

Trusted dependency and release automation PRs are waived by author login for
this attribution-only check because their metadata is controlled by repository
automation. This is not a build, test, or security waiver; see
[AUTOMATION-PR-GATES.md](AUTOMATION-PR-GATES.md). Human-authored PRs are not
waived; if the workflow fails, remove the reported marker from the PR title, PR
body, or offending commit message before merge.
