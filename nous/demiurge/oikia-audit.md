# theke/oikia/ Content Audit — Syl

**Auditor:** Syl (home domain)  
**Date:** 2026-02-10  
**Scope:** All files in theke/oikia/ (home — household, health, finance, inventory)

---

## Summary

**Total files:** 38 (14 markdown, 1 CSV data, 12 wardrobe markdown + 1 wardrobe CSV, 1 PDF, 8 finance CSVs + 1 zip archive, 1 README)

**Overall assessment:** oikia is correctly positioned in theke — it's shared family knowledge that multiple agents may reference. However, several files are stale "day one" scaffolding that were never fleshed out, and the wardrobe inventory has significant redundancy (markdown + CSV duplicating each other).

---

## File-by-File Assessment

### README.md
| Question | Answer |
|----------|--------|
| Belongs in theke? | ✅ Yes — vault navigation |
| Redundant? | No |
| Stale? | Partially — references `[[documents-README]]` and `[[ardent-llc/]]` which may be relocated |
| Automatable? | Dataview queries are already auto-generated |
| One clear job? | ✅ Yes — directory index |

**Recommendation:** **Keep** — update links if files move during broader audit.

---

### household-basics.md
| Question | Answer |
|----------|--------|
| Belongs in theke? | ⚠️ Borderline — it's half reference (family roster) and half agent onboarding notes ("Learn routines as we go", "Coordinate with Syn") |
| Redundant? | Yes — family roster duplicates USER.md and CONTEXT.md; "next steps" are stale |
| Stale? | ✅ Yes — dated 2026-01-28, "Next Steps" are all completed (calendar access exists, shared resources are set up) |
| Automatable? | No |
| One clear job? | No — mixes family reference with agent bootstrapping notes |

**Recommendation:** **Merge into household-operations.md** — pull the family roster into household-operations as the canonical "who lives here" section. Delete the onboarding/bootstrapping content (it was agent working memory from day one, not shared knowledge). The symlink in nous/syl/memory/ would need updating.

---

### household-operations.md
| Question | Answer |
|----------|--------|
| Belongs in theke? | ✅ Yes — operational household reference |
| Redundant? | Partially — pet info duplicates luna-meds.md; location duplicates what's in Letta |
| Stale? | ✅ Yes — almost entirely TODOs and placeholders from 2026-01-28. "Emergency Contacts: TODO", "Vet info: TBD", "Email access pending". These are 2+ weeks old and never updated. |
| Automatable? | No |
| One clear job? | Yes — household ops reference, but it's mostly empty scaffolding |

**Recommendation:** **Keep but overhaul** — this should be the canonical household operations file. Absorb family roster from household-basics.md. Fill in the TODOs or remove them. Remove agent-specific instructions ("Be proactive and self-directed") — those belong in SOUL.md/CONTEXT.md, not shared knowledge.

---

### cooper-detailed-schedule.md
| Question | Answer |
|----------|--------|
| Belongs in theke? | ✅ Yes — shared family knowledge about Cooper's care |
| Redundant? | Partially overlaps with cooper-development.md |
| Stale? | ⚠️ Aging — "10 months" was accurate in January, Cooper is now ~11 months. Schedule and wake windows need updating as he grows. |
| Automatable? | Partially — could be auto-populated from Google Calendar or Huckleberry data |
| One clear job? | ✅ Yes — daily care schedule and nutrition reference |

**Recommendation:** **Keep** — this is core operational knowledge. Consider merging with cooper-development.md (see below). Add a "last verified" date so staleness is visible. Cooper's age should be calculated, not hardcoded.

---

### cooper-development.md
| Question | Answer |
|----------|--------|
| Belongs in theke? | ✅ Yes — shared family knowledge |
| Redundant? | ⚠️ Significant overlap with cooper-detailed-schedule.md — both contain the daily schedule, nutrition info, formula details, iron sources, and meal plans |
| Stale? | Same aging issue — "10 months" references throughout |
| Automatable? | The Huckleberry data analysis section could be regenerated from the CSV |
| One clear job? | No — it's trying to be schedule + nutrition + development tracking + research citations all at once |

**Recommendation:** **Merge with cooper-detailed-schedule.md** into a single `cooper.md` file. Structure as: (1) Current Status, (2) Daily Schedule, (3) Nutrition, (4) Development Notes, (5) Research References. One file, one subject, clear sections. The current split creates drift where one file gets updated and the other doesn't.

---

### home-recipe-book.md
| Question | Answer |
|----------|--------|
| Belongs in theke? | ✅ Yes — shared household reference |
| Redundant? | No |
| Stale? | No — contains one recipe (dishwasher detergent), which is stable reference material |
| Automatable? | No |
| One clear job? | ✅ Yes — household recipes |

**Recommendation:** **Keep** as-is. Will grow naturally as recipes are added.

---

### amazon-buy-list.md
| Question | Answer |
|----------|--------|
| Belongs in theke? | ⚠️ Borderline — this is a transient shopping list, not persistent knowledge |
| Redundant? | No |
| Stale? | ⚠️ Yes — one item (vacuum filters), dated 2026-01-30. Likely either purchased or forgotten. |
| Automatable? | Could be managed via Taskwarrior or a shopping list tool instead |
| One clear job? | Yes — buy list |

**Recommendation:** **Move to nous/syl or manage via Taskwarrior** — transient shopping lists are working memory, not vault knowledge. Could be `tw add "Order Fantik filters" project:home.errands +amazon`. If kept, should have a review/purge cadence.

---

### local-resources.md
| Question | Answer |
|----------|--------|
| Belongs in theke? | ✅ Yes — stable local reference |
| Redundant? | No |
| Stale? | No — restaurants and medical facilities are stable |
| Automatable? | Could be enriched with live data (hours, ratings) but manual is fine |
| One clear job? | ✅ Yes — local family resources |

**Recommendation:** **Keep** — solid reference file. Consider adding: pediatrician info, pharmacy, Cooper's daycare details.

---

### luna-meds.md
| Question | Answer |
|----------|--------|
| Belongs in theke? | ✅ Yes — pet care reference |
| Redundant? | Partially — the "monthly meds" fact is also in household-operations.md |
| Stale? | ⚠️ Yes — "Next Due Dates" lists Feb/Mar/Apr 2026 but has no mechanism to auto-update. Should list the *medication names* and dosages, not just "meds." |
| Automatable? | ✅ Yes — monthly reminder is better handled by a cron job than a static file |
| One clear job? | Yes, but incomplete — what meds? What dosage? What vet? |

**Recommendation:** **Keep but flesh out** — add actual medication names, dosages, vet contact. Remove the "Next Due Dates" section (that's a calendar/cron concern, not a knowledge file). Consolidate the mention in household-operations.md to just point here.

---

### luna-repair-protocol.md
| Question | Answer |
|----------|--------|
| Belongs in theke? | ⚠️ Borderline — the general principle (put Luna outside for service workers) is shared knowledge; the specific Jan 29 event context is ephemeral |
| Redundant? | No |
| Stale? | ✅ Yes — "Today's Example (Jan 29)" is 12 days old. The principle is good, the example is stale. |
| Automatable? | No |
| One clear job? | Yes — Luna + service worker protocol |

**Recommendation:** **Keep but clean up** — rename to `luna-protocols.md`, remove the specific Jan 29 event, keep the general protocol. Could merge into luna-meds.md to create a single `luna.md` pet care file.

---

### huckleberry-data.csv
| Question | Answer |
|----------|--------|
| Belongs in theke? | ✅ Yes — historical family data |
| Redundant? | No |
| Stale? | ⚠️ Somewhat — last entry is 2026-01-21, suggesting Huckleberry tracking stopped ~3 weeks ago |
| Automatable? | Could be auto-imported if Huckleberry has an API/export schedule |
| One clear job? | ✅ Yes — Cooper tracking data |

**Recommendation:** **Keep** — 3,246 records of valuable historical data. Consider whether new data is being captured elsewhere now that Huckleberry isn't in use.

---

### documents-README.md
| Question | Answer |
|----------|--------|
| Belongs in theke? | ⚠️ Questionable — references `[[ardent-llc/]]` (business filings) and `[[health/]]`, but the ardent-llc folder doesn't appear to exist in oikia/ |
| Redundant? | Partially overlaps with README.md |
| Stale? | ✅ Yes — appears to be a leftover from a different vault structure |
| Automatable? | N/A |
| One clear job? | Was an index for a "documents" folder that may have been restructured |

**Recommendation:** **Delete** — orphaned index file. The README.md already covers the directory structure. The health/ and finance/ folders are linked from the main README.

---

### health/NeuropsychologicalEvaluation.pdf
| Question | Answer |
|----------|--------|
| Belongs in theke? | ✅ Yes — permanent medical record |
| Redundant? | No |
| Stale? | No — medical records are archival |
| Automatable? | No |
| One clear job? | ✅ Yes — medical document storage |

**Recommendation:** **Keep** — sensitive medical document, correctly stored. Consider whether it needs an index file or if the folder is self-documenting.

---

### finance/2025/*.csv + finance/2026/*.csv
| Question | Answer |
|----------|--------|
| Belongs in theke? | ✅ Yes — financial records are permanent reference |
| Redundant? | No |
| Stale? | No — archival data by nature |
| Automatable? | ✅ Yes — Relay bank statements could potentially be auto-imported |
| One clear job? | ✅ Yes — monthly bank statements |

**Recommendation:** **Keep** — 13 monthly Relay statements (Feb 2025–Feb 2026 partial). Well-organized by year. The archive/ zip contains older copies of the same CSVs and could be pruned once confirmed redundant.

---

### finance/archive/relay-statements-*.zip
| Question | Answer |
|----------|--------|
| Belongs in theke? | ⚠️ Questionable if individual CSVs already exist |
| Redundant? | ✅ Likely — appears to be the original bulk import, now unpacked into individual files |
| Stale? | N/A — archival |
| Automatable? | N/A |
| One clear job? | Backup |

**Recommendation:** **Delete or move to NAS** — if the individual CSVs cover the same data, this zip is redundant. Verify before removing.

---

### inventory-wardrobe/*.md (12 files) + wardrobe_inventory.csv
| Question | Answer |
|----------|--------|
| Belongs in theke? | ✅ Yes — permanent personal inventory |
| Redundant? | ✅ **Major redundancy** — every single item appears in both the individual markdown files AND the CSV. The CSV has 70 rows covering the exact same items as the 12 markdown files. Two complete copies of the same inventory in different formats. |
| Stale? | Some items have "TBA" details (Carhartt jackets, REI puffer, Smartwool, etc.) |
| Automatable? | ✅ The markdown files could be generated from the CSV, or vice versa |
| One clear job? | Each markdown file = one category. CSV = everything. But maintaining both is unsustainable. |

**Recommendation:** **Pick one format, generate the other.** The CSV is the better source of truth (structured, queryable, complete). Keep the CSV as canonical and either: (a) auto-generate the markdown files from it for browsing, or (b) drop the markdown files entirely and query the CSV directly. Currently, any wardrobe update requires editing in two places — that will drift.

---

## Priority Recommendations

### High Priority (structural issues)
1. **Merge cooper-detailed-schedule.md + cooper-development.md** → single `cooper.md` — eliminates major duplication, single source of truth
2. **Merge household-basics.md into household-operations.md** — basics is stale scaffolding, ops should be the canonical file
3. **Resolve wardrobe inventory duplication** — pick CSV or markdown as canonical, generate the other

### Medium Priority (cleanup)
4. **Delete documents-README.md** — orphaned index
5. **Clean luna-repair-protocol.md** — remove stale event, keep protocol
6. **Flesh out luna-meds.md** — add actual medication details
7. **Move amazon-buy-list.md to working memory** — transient, not knowledge

### Low Priority (maintenance)
8. **Overhaul household-operations.md** — fill TODOs or remove them
9. **Verify finance archive zip redundancy** — delete if individual CSVs cover it
10. **Add "last verified" dates** to Cooper files — age-sensitive content needs visible freshness

### Symlink Note
My workspace (nous/syl/memory/) has symlinks pointing to 8 theke/oikia files. Any merges/renames/deletes above will need symlink updates. This is good architecture — the symlinks confirm these files are shared knowledge that agents reference, not agent-specific working memory.

---

*Audit complete. Ready for Demiurge consolidation.*
