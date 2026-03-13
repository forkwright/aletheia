# PII history rewrite

Documents the git history rewrite performed to remove personally identifiable
information from the Aletheia repository prior to public release.

## What was found

### Author/committer fields
The following identities contained PII (real names, work emails, business names):

| Identity | PII Type |
|----------|----------|
| `REDACTED <REDACTED@employer.example>` | Real surname + work email |
| `REDACTED <REDACTED@fork.example>` | Real full name |
| `REDACTED <REDACTED@personal.example>` | Real name in email |
| `admin <admin@business.example>` | Business name in email |

Non-PII pseudonymous identities (`Aletheia`, `Syn`, `Demiurge`) were also
consolidated for consistency.

### Co-authored-by trailers
Multiple commit messages contained `Co-authored-by:` trailers with real names
and emails. All were removed.

### Commit message bodies
Some commit messages referenced:
- `REDACTED/aletheia` (GitHub username with real surname)
- `/home/REDACTED/` (home directory paths)
- `employer.example` (employer name)
- `business.example` (business name)

### pii-patterns.txt
`.github/pii-patterns.txt` contained a regex pattern matching a real surname.
Genericized to `[A-Za-z]{3,}ample\b`.

## What was changed

### Tool
`git filter-repo` with `--mailmap` and `--message-callback`.

### Identity consolidation (.mailmap)
All human identities were mapped to: `Cody <cody@aletheia.dev>`

Bot identities were preserved:
- `dependabot[bot] <49699333+dependabot[bot]@users.noreply.github.com>`
- `github-actions[bot] <41898282+github-actions[bot]@users.noreply.github.com>`
- `GitHub <noreply@github.com>` (committer for web merges)

### Message rewrites
| Pattern | Replacement |
|---------|-------------|
| `Co-authored-by:` trailers | Removed |
| `REDACTED@employer.example` | `cody@aletheia.dev` |
| `REDACTED@personal.example` | `cody@aletheia.dev` |
| `REDACTED@fork.example` | `cody@aletheia.dev` |
| `admin@business.example` | `cody@aletheia.dev` |
| `@employer.example` | `@example.corp` |
| `employer.example` (word boundary) | `example.corp` |
| `employer` (word boundary) | `ExampleCo` |
| `business.example` | `example-business` |
| `REDACTED/` (GitHub username) | `forkwright/` |
| `/home/REDACTED/` | `/home/cody/` |
| `REDACTED` (username) | `cody` |
| `REDACTED` (surname, word boundary) | `Cody` |

### File changes
- `.github/pii-patterns.txt`: surname-specific regex → `[A-Za-z]{3,}ample\b`

## How to verify

```bash
# No PII in author/committer fields
git log --all --format='%an <%ae> %cn <%ce>' | sort -u

# No co-authored-by trailers
git log --all --format='%b' | grep -i 'co-authored-by' | sort -u

# No PII in commit messages
git log --all --format='%B' | grep -iE '(employer\.example|business\.example)' | head -20
```

## Impact

All commit SHAs have changed. This is a full history rewrite. After review,
the repo owner must force-push to main.
