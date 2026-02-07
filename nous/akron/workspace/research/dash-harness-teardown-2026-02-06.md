# Dash Harness Teardown & Component Map
**Date:** 2026-02-06
**Vehicle:** 1997 Dodge Ram 2500 (VIN: 3B7KF23D9VM592245)
**Status:** Full harness removed from dash frame, unwrapped, being traced

---

## Modules Identified

| Module | Part Number | Location on Frame | Connector | Keep/Remove |
|---|---|---|---|---|
| PCM / ECM | TBD (check label) | Center dash frame | Single large multi-pin, bottom | **KEEP** — transmission, fuel shutoff, grid heaters, cruise |
| CTM (Central Timer Module) | TBD | Driver side, near column | Single multi-pin | **KEEP** — wipers, chimes, dome lights, power locks |
| Buzzer / Chime | 56006952 | Next to PCM on bracket | Multi-pin + electromagnetic coil | **REMOVE** — annoying, non-critical. Pull fuse #11 |

---

## Ground Points

| # | Location | Hardware | Wires | Notes |
|---|---|---|---|---|
| 1 | Right side, glovebox area | 5/16 nut, ring terminal, star washer | Single ring terminal | Clean to bare metal, dielectric grease around contact |
| TBD | More to be documented | | | |

**Ground Rules:**
- Factory grounds stay at factory locations — do NOT consolidate
- Clean to bare metal, star washer, dielectric grease AROUND (not between) contact faces
- Star ground topology for new aux circuits only (Phase 3)
- Plan: Add 1x AUX GND point (10 AWG marine, labeled) for future dash accessories

---

## SRS Wiring — REMOVED

### ASDM (Airbag System Diagnostic Module)
- **Was located:** Center floorboard, under dash, SRS-labeled plastic cover
- **Photo confirmed:** Aug 3, 2025 — both connectors into same module
- **Module:** Already removed (no airbag system on truck)
- **Contains:** Internal crash sensor, driver/passenger squib controllers, warning lamp, diagnostic

### Yellow Connector — ✅ REMOVED IN ENTIRETY
- **Connector:** Yellow, 4-position plug, wires in right 2 slots
- **Wires:** Solid green + black/blue stripe (twisted pair)
- **Function:** Airbag squib circuit (driver side, to clockspring)
- **Routing:** Did NOT go to fuse box. Connected to another harness section (clockspring harness). Terminated at a connector junction.
- **Previously cut** at clockspring end when old clockspring was removed
- **Status:** Pulled completely out of harness. On garage floor. Gone.

### Black Connector — ✅ ALL 10 WIRES REMOVED
- **Connector:** Black, 10-pin
- **All wires traced, cut, heat shrunk, taped, and temp labeled.**

| Row | Position | Color | Traced To | Method | Status |
|---|---|---|---|---|---|
| 1 | 1 | Black / light blue stripe | White bulkhead junction (ASDM) | Cut + heat shrink stub | ✅ DONE |
| 1 | 2 | Peach / brown | White bulkhead junction (ASDM) | Cut + heat shrink stub | ✅ DONE |
| 1 | 3 | Black / orange stripe | White bulkhead junction (ASDM). Likely A22 (IGN RUN) or Z3 (ground) | Cut + heat shrink stub | ✅ DONE |
| 1 | 4 | Black / brown stripe | Dash cluster harness (white connector) — SRS warning lamp signal | Cut + heat shrink stub | ✅ DONE |
| 2 | 1 | Blue / light blue stripe | White bulkhead junction (ASDM) | Cut + heat shrink stub | ✅ DONE |
| 2 | 2 | Light blue | White bulkhead junction (ASDM) | Cut + heat shrink stub | ✅ DONE |
| 2 | 3 | White / black stripe | DLC (OBD-II port) — CCD Bus (-). Twisted pair with purple | Cut + heat shrink stub | ✅ DONE |
| 2 | 4 | Purple / faint black-grey stripe | DLC (OBD-II port) — CCD Bus (+). Twisted pair with white/black | Cut + heat shrink stub | ✅ DONE |
| 2 | 5 | Blue / yellow stripe | Fuse box, Fuse #12 AIRBAG CLUSTER — SRS power feed | Yanked entirely | ✅ DONE |
| 2 | 6 | Green / yellow stripe | Splice junction → fuse #8 WIPER + dash harness. **Shared IGN ST-RUN circuit** — SRS branch only removed | Cut at SRS end + heat shrink | ✅ DONE |

**Routing summary:**
- 5 wires → white bulkhead junction (all SRS-only, to ASDM location)
- 2 wires → DLC/OBD-II port (CCD bus pair, ASDM branch only)
- 1 wire → fuse #12 (SRS power, yanked)
- 1 wire → dash cluster harness (SRS warning lamp)
- 1 wire → shared splice (SRS branch removed, wiper circuit preserved)

### BK/OR Wire Research (2026-02-06)
**Source:** 1998 Ram FSM (spillage.net PDF, 2356 pages), Section 8W-43 Airbag System + 8W-80 Connector Pinouts

**1998 ACM (Airbag Control Module) — Full 23-pin Pinout:**
| CAV | Circuit | Wire Color | Function |
|---|---|---|---|
| 4 | Z6 18 | BK/PK | Ground |
| 5 | R43 18 | BK/LB | Driver Airbag Line 1 |
| 6 | R45 18 | DG/LB | Driver Airbag Line 2 |
| 7 | R142 18 | BK/YL | Passenger Airbag Line 1 |
| 8 | R144 18 | DG/YL | Passenger Airbag Line 2 |
| 14 | F14 18 | LG/YL | Fused IGN (ST-RUN) |
| 15 | F23 18 | DB/YL | Fused IGN (RUN) |
| 16 | G111 18 | LG/BK | SBCM Fault Signal |
| 21 | D1 18 | VT/BR | CCD Bus (+) |
| 22 | D2 18 | WT/BK | CCD Bus (-) |
| 1-3, 9-13, 17-20, 23 | - | - | Empty |

**Key finding: BK/OR does NOT appear in the 1998 ACM pinout.**

**Three circuits use BK/OR in the 1998 FSM:**
1. **A22 14 BK/OR** — Ignition Switch Output (RUN) — power distribution through C134 junction
2. **L76 12-14 BK/OR** — Trailer Park Lamps (trailer tow relay output)
3. **Z3 12-16 BK/OR** — Ground (general instrument panel ground)

**Assessment:** The BK/OR wire in the SRS black connector is most likely:
- **A22 (IGN RUN power feed to ASDM)** — plausible, as the 1997 ASDM needed ignition power
- **Z3 (ground)** — possible, though the 1998 uses BK/PK (Z6) for ACM ground
- The 1997 ASDM may have a different pinout than the 1998 ACM (different module generation)
- Either way: it's safe to cut since SRS system is fully removed

**Important note:** The 1997 uses the earlier **ASDM** (Airbag System Diagnostic Module) while 1998 uses the **ACM** (Airbag Control Module) — potentially different connectors and pinouts.

**⚠️ VERIFY: If this was A22 (RUN power), cutting it would only affect the now-removed ASDM. No other systems share this specific wire run.**

---

## Fuses — Status Changes

| Fuse # | Rating | Circuit | Action | Reason |
|---|---|---|---|---|
| 11 | 10A | BUZZER CONSOL | **PULL** | Buzzer module removed |
| 12 | 15A | AIRBAG CLUSTER | **PULL** | SRS system removed |
| 16 | 15A | AIRBAG | **PULL** | SRS system removed |
| 18 | 15A | PARK LAMP | **BLOWN** — dead short downstream of headlight switch | Diagnose after reassembly |

---

## Dash Frame Treatment
- Harness and fuse box fully removed from frame
- Frame stripped to bare metal + aluminum side panels
- Bulk rust removed with wire brush (drill-mounted)
- **Rust-Oleum Rust Reformer** applied to all steel surfaces
  - NOT on aluminum mounting panels
  - NOT on ground contact points (masked)
  - 20-40 min touch dry, 24 hr full cure

---

## Harness Condition Notes
- Factory harness wrap: original fabric tape, deteriorated in places
- Will rewrap entirely with X-Fasten fleece harness tape
- Split loom to be added where wires cross metal edges
- Exposed copper found at old clockspring cut — needs heat shrink
- Blue butt connectors on radio wiring (previous owner) — need redo with solder + heat shrink (future)

---

## Photos Referenced
- Aug 3, 2025: ASDM module in situ (both connectors confirmed)
- Feb 6, 2026: Ground point #1, SRS connectors, CTM, PCM, buzzer module, harness unwrapped, yellow SRS wires removed, frame stripped

---

*Document will be updated as black connector wires are traced.*
*For electrical prep plan while harness is out, see: `harness-out-electrical-prep.md`*
