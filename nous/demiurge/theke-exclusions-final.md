# Theke Vault - Obsidian Sync Exclusions

## Recommended Exclusions (>10MB or Binary Heavy)

### Large Directories to Exclude

```
_reference/
poiesis/imaging/
poiesis/cad/
akron/royal_enfield_gt650/photos/
akron/royal_enfield_gt650/manuals/
akron/royal_enfield_gt650/manual_processing/
akron/dodge_ram_2500_1997/photos/
akron/dodge_ram_2500_1997/manuals/  
akron/dodge_ram_2500_1997/manual_processing/
akron/database/
chrematistike/fa25/
```

### Size Breakdown (Post-Restructure)

| Domain | Total Size | MD Files | Heavy Content | Sync Status |
|--------|------------|----------|---------------|-------------|
| akron | 3.5G | 79 | Vehicle DBs, photos, manuals | Partial sync |
| ardent | 109M | 107 | Clean | Full sync ✓ |
| autarkeia | 1.2G | 27 | Unknown binary content | Needs audit |
| chrematistike | 1.4G | 226 | fa25/ old semester (1.3G) | Partial sync |
| ekphrasis | 6.6M | 64 | Clean | Full sync ✓ |
| epimeleia | 444K | 19 | Clean | Full sync ✓ |
| metaxynoesis | 228K | 14 | Clean | Full sync ✓ |
| oikia | 4.3M | 22 | Clean | Full sync ✓ |
| poiesis | 2.3G | 52 | imaging/ (2.2G), cad/ (116M) | Partial sync |
| _reference | 5.0G | 27 | Library/offline docs | Exclude entirely |
| summus | 569M | 458 | Some binary content | Needs audit |
| _templates | 36K | 6 | Clean | Full sync ✓ |

### Calculated Sync Footprint

**With exclusions applied:**
- **Full sync domains**: ardent (109M) + ekphrasis (6.6M) + epimeleia (444K) + metaxynoesis (228K) + oikia (4.3M) + _templates (36K) = ~121M
- **Partial sync domains**: 
  - akron: ~250M (without database, photos, manual_processing)
  - chrematistike: ~100M (without fa25/)
  - poiesis: ~320K (without imaging/, cad/)
  - summus: ~569M (needs further audit)

**Total estimated sync size: ~1.04GB** (down from ~17GB total)

### Exclusion Rationale

1. **_reference/** (5.0G) - Offline reference materials, explicitly excluded by design
2. **poiesis/imaging/** (2.2G) - AI art generation outputs, binary heavy
3. **poiesis/cad/** (116M) - 3D models and STL files, binary heavy  
4. **akron vehicle folders** (3.1G) - Photos, manuals, processing outputs
5. **akron/database/** (517M) - Structured data files
6. **chrematistike/fa25/** (1.3G) - Completed semester, archived

### Notes

- Autarkeia (1.2G) needs audit - unclear what's causing size
- Summus (569M) may have excludable content in _outputs/ or archives
- All markdown content preserved and synced
- Heavy binary content accessible locally or via alternative sync

*Generated: 2026-02-10 post-restructure*