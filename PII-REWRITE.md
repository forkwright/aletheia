# PII History Rewrite

Documents the git history rewrite performed to remove personally identifiable
information from the Aletheia repository prior to public release.

## What Was Found

### Author/Committer Fields
The following identities contained PII (real names, work emails, business names):

| Identity | PII Type |
|----------|----------|
| `CKickertz <ckickertz@summusglobal.com>` | Real surname + work email |
| `Cody Kickertz <cody@forkwright.com>` | Real full name |
| `forkwright <cody.kickertz@pm.me>` | Real name in email |
| `admin <admin@ardentleatherworks.com>` | Business name in email |

Non-PII pseudonymous identities (`Aletheia`, `Syn`, `Demiurge`) were also
consolidated for consistency.

### Co-Authored-By Trailers
Multiple commit messages contained `Co-authored-by:` trailers with real names
and emails. All were removed.

### Commit Message Bodies
Some commit messages referenced:
- `CKickertz/aletheia` (GitHub username with real surname)
- `/home/ckickertz/` (home directory paths)
- `summusglobal` / `Summus` (employer name)
- `ardentleatherworks` (business name)

### pii-patterns.txt
`.github/pii-patterns.txt` contained a regex pattern `[A-Za-z]{3,}ertz\b`
designed to match the real surname. Genericized to `[A-Za-z]{3,}ample\b`.

## What Was Changed

### Tool
`git filter-repo` with `--mailmap` and `--message-callback`.

### Identity Consolidation (.mailmap)
All human identities were mapped to: `Cody <cody@aletheia.dev>`

Bot identities were preserved:
- `dependabot[bot] <49699333+dependabot[bot]@users.noreply.github.com>`
- `github-actions[bot] <41898282+github-actions[bot]@users.noreply.github.com>`
- `GitHub <noreply@github.com>` (committer for web merges)

### Message Rewrites
| Pattern | Replacement |
|---------|-------------|
| `Co-authored-by:` trailers | Removed |
| `ckickertz@summusglobal.com` | `cody@aletheia.dev` |
| `cody.kickertz@pm.me` | `cody@aletheia.dev` |
| `cody@forkwright.com` | `cody@aletheia.dev` |
| `admin@ardentleatherworks.com` | `cody@aletheia.dev` |
| `@summusglobal.com` | `@example.corp` |
| `summusglobal` | `example.corp` |
| `summus` (word boundary) | `ExampleCo` |
| `ardentleatherworks` | `example-business` |
| `CKickertz/` | `forkwright/` |
| `/home/ckickertz/` | `/home/cody/` |
| `ckickertz` | `cody` |
| `kickertz` (word boundary) | `Cody` |

### File Changes
- `.github/pii-patterns.txt`: `[A-Za-z]{3,}ertz\b` → `[A-Za-z]{3,}ample\b`

## How to Verify

```bash
# No PII in author/committer fields
git log --all --format='%an <%ae> %cn <%ce>' | sort -u

# No co-authored-by trailers
git log --all --format='%b' | grep -i 'co-authored-by' | sort -u

# No PII in commit messages
git log --all --format='%B' | grep -iE '(summus|kickertz|ckickertz|ardentleather)' | head -20
```

## Impact

All commit SHAs have changed. This is a full history rewrite. After review,
the repo owner must force-push to main.
