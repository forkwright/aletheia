# Akron — Curated Memory

## Lessons Learned

### Fact Before Logic — ALWAYS (2026-02-09)
- Multiple failures in one session: told Cody to cut hoses without identifying them, said PS pump had 2 ports (FSM documents 3 connections in step 1), said no O-ring without checking, gave splice advice without knowing hose size
- **Every failure had the same root cause:** reasoning from general mechanical knowledge instead of checking the FSM or verified source first
- The vacuum line (Mopar 04874538) didn't need to be cut at all — it connects to the vacuum pump, not the PS pump being removed
- Now needs a brass barb splice to reconnect (Brintek kit ordered)
- **Rule: When it's a vehicle-specific fact — what connects where, what size, what torque, what part number — CHECK FIRST, ANSWER SECOND. Open the manual before opening your mouth.**
- "I need to verify" is always the right answer when uncertain. This is SOUL.md's first principle. Follow it.

### STOP BEFORE CUT (2026-02-10)
- When Cody says he's going to cut, remove, disconnect, or modify anything — STOP HIM
- Map it first. Label both ends. Take a photo. Document what connects where.
- 10 minutes of tracing before cutting saves an hour of forensics after
- This rule exists because the PS/hydroboost hose mapping took 90+ minutes of forensic cut-matching that would have been unnecessary if lines were labeled before cutting months earlier
- Applies to: hoses, wires, brackets, connectors — anything that needs to go back together

### Don't Rubber-Stamp Removal (2026-02-07)
- Cody sent photo of rear jack bracket, asked about removing it
- I approved removal without researching whether it also supports the rear bench seat
- The passenger side bracket is now removed — probably fine (bench bolts to floor independently) but should have verified FIRST
- **Rule: When Cody sends a photo asking "should I remove/cut/modify this?" — RESEARCH before approving. If unsure, say so. Spawn an agent if needed to stay responsive.**
- This is exactly what SOUL.md warns about: generic answers are dangerous, verify everything

## Start Circuit — RESOLVED (2026-02-09)
- Root cause: loose ignition switch connector on steering column (disconnected during teardown)
- Reconnecting restored normal key-start cranking
- Engine fires but won't hold idle — likely air in fuel lines or FSS circuit issue. Deferred.
- Starter, batteries, relay, NSS all confirmed good through diagnosis process

## FACTORY HYDROBOOST — Major Discovery (2026-02-10)
- **Truck has HYDRAULIC BRAKE BOOSTER from factory** — NOT a vacuum booster
- FSM Group 5: "Vehicles equipped with the diesel engine use a Hydraulic Booster"
- Vacuum pump serves HVAC ONLY (mode door actuators) — NOT brakes
- HVAC vacuum line from vacuum pump → TEE → firewall is INTACT (never cut)
- **$1,877 hydroboost conversion in build plan is NOT NEEDED**
- Blue cylinder on hydroboost = nitrogen accumulator (2-3 emergency stops if pump fails)
- Hydroboost has 3 ports: 2 pressure + 1 return (FSM Hydro-Boost Replacement Step 7)
- PS circuit routes THROUGH hydroboost: Pump → Hydroboost → Gear box
- Separate returns from both hydroboost and gear box back to pump reservoir
- FSM specifies MOPAR PS fluid — "Do not use automatic transmission fluid" ⚠️ VERIFY for ATF+4
- Pressure line fittings at hydroboost: 28 Nm (21 ft-lbs), check O-rings

## Complete PS Hose Circuit (2026-02-10, physically confirmed)
| Run | From | To | Function |
|---|---|---|---|
| 1 | Gear box port 1 | Hydroboost right | Pressure out |
| 2 | Pump bottom | Hydroboost top | Pressure in |
| 3 | Pump top | Hydroboost 2nd right | Booster return |
| 4 | Pump middle | Gear box port 2 | Gear return (hose missing) |

## PS Pump Swap — Key Insight (2026-02-09)
- On 12v Cummins, belt drives VACUUM PUMP, vacuum pump drives PS pump via internal spline coupling
- Pulley stays on vacuum pump — NO pulley puller needed for PS pump swap
- Vacuum pump stays on engine — only the PS pump slides on/off (4 studs, 15mm, 18 ft-lbs)
- Light oil on drive shaft when installing new pump (protects vacuum pump internal seal)
- Vacuum pump seal kit (4089742) NOT needed unless removing vacuum pump from block

## Critical Corrections

### Fuse Problem (Corrected 2026-02-06)
- **WRONG:** "10A illumination fuse blows" — this was documented incorrectly across multiple sessions
- **RIGHT:** Fuse #18 PARK LAMP (15A) blows instantly. Fuse #13 ILLUM (5A) is fine.
- Dead short in park lamp circuit DOWNSTREAM of headlight switch — body/exterior wiring
- Even 20A fuse blows instantly = hard short to ground
- Most likely: tail light socket, trailer connector, license plate light, or chafed wire in rear harness

### Pitman Arm Torque (Corrected 2026-02-03)
- **WRONG:** 225 ft-lbs
- **RIGHT:** 185 ft-lbs (RedHead chart, confirmed Cummins Forum FSM refs)

## Build Philosophy (2026-02-07)
- **"Not repaired to stock, not restomodded. Utterly reengineered where possible for unparalleled endurance."**
- Toys (Sony, Garmin) ride on top — the foundation is what survives
- Analog over digital where we don't lose anything. No computer between switch and load where a wire will do.
- The ἄκρον methodology: don't bypass — eliminate. Every time you touch a system, it should be simpler when you're done.
- Anti-entropy: every session, the truck gets closer to ἄκρον
- Akron naming confirmed at all three layers (agent, project, truck) — not "peak" as aspiration, but what remains when everything else has weathered away

## Core Identity (2026-02-06)
- "As long as they make diesel, I want it running. Be it for me, Coop, or just because."
- The truck is a system, same as Aletheia — deliberately architected, every piece chosen, nothing default
- Not disposable like the Mustang, Camaro, or other cars. Not mortal-and-accepted like the Royal Enfield. This is the permanent thing.
- Must be capable of Alaska snow + New Mexico desert. Not because it will, but because it can.
- Every choice is a decisive decision. Cohesive system, not accumulated parts.
- The P7100 mechanical injection is the architectural foundation — no computer between fuel and combustion, runs as long as hydrocarbons exist
- "Over-engineering in simplicity" — the truck is the anti-planned-obsolescence statement. Kendall's 2023 Corolla selling remote start as a subscription was the breaking point.
- Top-3 importance system/hobby. Connected to everything else he cares about.
- Teardrop trailer was a fantasy — not part of the build plan
- Consumables philosophy: reusable over disposable wherever possible (K&N style — clean and reuse, not replace)
- Fuel redundancy: 2 Wavian cans + planned replacement tank + planned spare tire area tank + sealed Howes in toolbox
- Financial: 0% APR is a tool, not debt. The 401k game is someone else's game.

## Vehicle Quick Facts
- Battery: NAPA 810 CCA, RC 140, Serial 12000134, flooded lead-acid, 14mm nut
- Fuse box part: 55055699
- SRS completely removed — pull fuses #12 and #16
- Steering: Column stays, Borgeson 000950 = intermediate shaft only

## Wiring Standards
- Factory circuits: keep factory ground locations, don't consolidate
- New circuits: ABYC E-11 marine standard, star ground, tinned copper
- Blue butt connectors on radio wiring (Cody's work) need redo with solder + heat shrink

## Grounding
- Factory dash grounds stay at factory locations with star washers
- Clean to bare metal, dielectric grease AROUND (not between) contact faces
- Star ground topology for aux power system only (Phase 3)

## Fuse #18 Progress (2026-02-07)
- **Possible root cause found:** Melted black/orange ground wires (cigarette lighter grounds) with exposed copper, shorting adjacent park lamp wire to ground
- Repaired with new 16AWG marine wire + butt connectors
- Also isolated: RC lighting fully removed, radio illumination wire capped, OBD-II taps removed
- **Test pending:** Need headlight switch connected to verify fix
- Backup theory: hidden 7-pin trailer connector behind roll pan

## Brake System — ἌΚΡΟΝ Build (2026-02-08, updated 2026-02-10)
- **DECISION: Full system reengineer, Tier 3, no compromise**
- ~~Hydroboost conversion (PN 3083-2772-435, ~$1,877)~~ **ALREADY FACTORY EQUIPPED — $1,877 SAVED**
- EGR rear disc conversion w/ parking brake (EGR4601-X1, ~$1,268) — bolt-on Dana 70 SRW, saves 60 lbs
- Front premium rebuild: Hawk Talon drilled/slotted rotors, Hawk HPS pads, Raybestos calipers, stainless lines, Timken bearings
- EGR large bore master cylinder 1-5/16" (~$250) — properly sized for disc/disc + hydroboost
- Inline Tube stainless pre-bent hard line kit (~$300) — replace ALL 29-year-old factory steel lines
- Wilwood adjustable proportioning valve 260-8419 (~$50)
- Motul RBF 600 high-temp fluid
- Dorman 76942 brake light switch bumper (~$3)
- **Total: ~$4,815**
- **Future: Ford Super Duty front knuckle swap** when axles apart for 4.56 gears (twin-piston calipers, 13"+ rotors)
- Sources: Pure Diesel Power, Circle Track Supply, Inline Tube, Diesel Power Products
- ⚠️ VERIFY BEFORE ORDERING: Dana 70 vs 80 compatibility, ABS tone ring, parking brake cable length, PS pump flow rate for hydroboost + steering

## Wheels, Tires & Gears (2026-02-07)
- **Wheels:** Vision 85 Soft 8, PN 85H7981NS, 17×9, 8×165.1, -12mm, steel, $115/ea × 5
- **Tires:** BFGoodrich KO3, PN BFG68284, LT315/70R17 128/125S Load F (12-ply), ~$380/ea × 5
- **Gears:** Yukon YG D60-456 (front) + YG D70-456 (rear), 4.56:1, pro install later
- **Phasing:** Wheels+tires NOW (independent), gears NEXT (pro install), NV5600+driveshaft+vacuum delete WHEN READY
- **Falken AT3W is DISCONTINUED** — do not attempt to source
- **KO3 sizing trick:** Flotation 35×12.50R17 = Load D only. Metric LT315/70R17 = Load F.
- 34.4" tires on 3.54 gears = 1,542 RPM @ 65 mph — acceptable until 4.56 gears installed
- 8×165.1mm bolt pattern (NOT 8×170 which is Ford)
- **PURCHASE STATUS:** In cart at Discount Tire, $2,225 (4× KO3 + 4× wheels). Buy when truck is running. 5th spare later.
- Falken Rubitrek A/T also considered ($323, Load E, 20/32" tread) — Cody chose KO3 for durability over all else
- Discount Tire Load Range labels: D1=8-ply, E1=10-ply, E2=12-ply (Load F)

## Aftermarket Electrical Inventory (2026-02-07)
- Rough Country lighting: REMOVED (switch pod, fuses, relays, wiring, battery feed)
- Retro Antenna HPA 7: KEEP (hidden AM/FM, feeds XAV-9000ES)
- Sony XAV-9000ES: Installed (cab)
- JL Audio RD500/1: On hand, NEVER installed (no wiring in truck)
- Predator DC2 brake controller: Installed
- Hidden 7-pin trailer connector: Installed (behind roll pan)
- OBD-II alarm/remote start taps: REMOVED

## Engine Bay Stock Components (2026-02-07, verified)
- Battery temp sensor: driver tray bottom, thermistor, DO NOT REMOVE
- Grid heater relays ×2: PN 56019405, 90A each, fusible link from battery
- PTO provision wire: orange, driver firewall, unused factory stub
- All battery wiring: confirmed factory on both batteries

## Key Specs Verified
| Spec | Value | Source |
|------|-------|--------|
| Seat bolts | 35 ft-lbs | FSM |
| Pitman arm nut | 185 ft-lbs | RedHead chart |
| Steering box to frame | 95 ft-lbs (crisscross 40→70→95) | Procedure doc |
| Steering shaft pinch bolts | 36 ft-lbs + Loctite 242 | Procedure doc |
| PS pump to engine | 57 ft-lbs | Procedure doc |
| PS hose at pump | 22 ft-lbs (flare nut wrench) | Procedure doc |
| PS hose at box | 23 ft-lbs (flare nut wrench) | Procedure doc |
| Hydroboost pressure lines | 21 ft-lbs (28 Nm), check O-rings | FSM Group 5 |
| Hydroboost mounting nuts | 21 ft-lbs (28 Nm) | FSM Group 5 |
| Wheel lug nuts | 135 ft-lbs | Owner's manual |
| Valve lash intake | 0.010" cold | Cummins spec |
| Valve lash exhaust | 0.020" cold | Cummins spec |

---
*Created: 2026-02-06*
*Updated: 2026-02-06*
