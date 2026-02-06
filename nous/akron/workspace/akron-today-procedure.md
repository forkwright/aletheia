# AKRON — Today's Work Procedure
## February 6, 2026 — Interior Assembly + Illumination Short Diagnostic

**Goal:** Carpet down, LRB dash installed, clockspring on column, seats in. Diagnostic on 10A illumination circuit while dash components are accessible.

**Tools on hand:** Simpson 260-8P analog VOM, standard hand tools, rivet gun

---

## REFERENCE INDEX

| Task | Haynes #30041 | FSM Section |
|------|---------------|-------------|
| Carpet installation | Ch. 11, Sec. 25 (Floor covering) | Group 23: Body |
| Dash installation | Ch. 11, Sec. 26 (Instrument panel) | Group 23: Body — Instrument Panel |
| Seat installation | Ch. 11, Sec. 27 (Seats) | Group 23: Body — Seats |
| Clockspring | Ch. 12, Sec. 21 (Airbag/SRS) | Group 8U: Steering Column |
| Illumination circuit | Ch. 12, Sec. 4 (Fuses/circuits) | Group 8W: Wiring Diagrams |
| Steering column | Ch. 10, Sec. 12 (Steering column) | Group 19: Steering |

---

## PART 1: ILLUMINATION SHORT DIAGNOSTIC (Do First)

**Why first:** The dash is out. The wiring is exposed. This is the best access you'll ever have. Diagnose BEFORE you button everything up.

### What We Know
- Illumination fuse blows — **Fuse #13, 5A** (1996-1997 panel position)
- ⚠️ **CORRECTION:** Previously documented as "10A" — verified as **5A** per FSM fuse chart
- Headlight switch tested OK
- Rear lights disconnected — still blows
- Driver seat harness checked OK
- Radio disconnected — still blows
- **Suspected:** Behind-dash wiring, aftermarket equipment, or instrument cluster

### What's on the Illumination Circuit (Fuse #13)
Per FSM Group 8W / Haynes Ch. 12:

- Instrument cluster backlighting
- HVAC control panel lighting
- Radio display dimmer wire
- Fog lamp switch illumination
- Overdrive switch illumination
- Overhead console illumination (if equipped)
- A/C heater control illumination

### Diagnostic Procedure — Simpson 260-8P

**⚠️ BATTERY MUST BE DISCONNECTED FOR RESISTANCE TESTS**

#### Step 1: Baseline Resistance Test
1. Disconnect both battery negatives
2. Set Simpson 260-8P to **R×1** scale
3. Zero the meter (leads together, adjust)
4. At the fuse box, locate fuse #13 terminals
5. Place one lead on the LOAD side of fuse #13
6. Place other lead on a known good GROUND (bare metal on steering column bracket or dash frame bolt)
7. **Read resistance:**
   - **∞ (no deflection):** No short at this point — problem is downstream
   - **Near 0Ω (full deflection):** Dead short to ground exists on this circuit
   - **Some resistance (partial deflection):** Possible high-resistance fault

**Record reading: ____________ Ω**

#### Step 2: Isolate by Disconnection
With meter still connected to fuse #13 load side and ground:

**Disconnect one at a time. After each, re-check resistance.**

| # | Disconnect This | Resistance After | Short Here? |
|---|----------------|-----------------|-------------|
| 1 | Instrument cluster connector (large plug behind cluster) | _____ Ω | Y / N |
| 2 | HVAC control panel connector | _____ Ω | Y / N |
| 3 | Any aftermarket splices visible in harness | _____ Ω | Y / N |
| 4 | Overhead console connector (if present) | _____ Ω | Y / N |
| 5 | Fog lamp switch connector | _____ Ω | Y / N |
| 6 | Headlight switch illumination wire (thin wire, not main power) | _____ Ω | Y / N |

**When resistance jumps to ∞ after disconnecting something → THAT component/branch has the short.**

#### Step 3: Inspect the Guilty Branch
Once isolated:
- Visually trace that wire run — look for:
  - Bare copper touching metal (chafed insulation)
  - Melted insulation
  - Rodent damage
  - Aftermarket taps/splices done poorly
  - Pinched wires behind brackets or through grommets
- If the component itself is shorted (cluster, HVAC panel), test it separately:
  - Measure resistance across the component's illumination pins to ground
  - Near 0Ω = internal short in the component

#### Step 4: Also Test Dash Components While They're Out
The LRB dash isn't installed yet. Before it goes in:

- **Instrument cluster** (if being reused): Measure resistance from illumination supply pin to cluster ground pin. Should be >10Ω (bulb resistance). Near 0 = bad cluster.
- **HVAC control head**: Same test on its illumination pins.
- **Any switches** going back in: Check illumination wire to ground.

#### Step 5: Verify Fix
1. Reconnect everything
2. Install a KNOWN GOOD 5A fuse
3. Reconnect battery
4. Turn headlight switch to PARK position (activates illumination circuit)
5. Fuse holds = fixed
6. Fuse blows = still have a problem — repeat isolation

### If You Can't Find It Today
That's fine. Install a 5A fuse, see if it holds with the new dash. The short may have been in wiring that's no longer routed the same way. Document what you tested.

---

## PART 2: LAY CARPET

**Haynes Ch. 11, Sec. 25 | FSM Group 23**

### Procedure
1. Confirm soundproofing is fully adhered and not lifting at edges
2. Position carpet — start from firewall, work rearward
3. Cut holes for:
   - Steering column pass-through
   - Seat mounting bolt holes (4 per seat — feel for studs from underneath)
   - Seat belt anchor bolts
   - Any floor-mounted controls (dimmer switch if floor-mounted, parking brake)
4. Tuck edges:
   - Under door sill plates (when they go back)
   - Under dash at firewall
   - Around center console area
   - At rear wall behind seat
5. Use a sharp utility knife — cut from underneath where possible for clean edges
6. **Don't trim too aggressively** — you can always cut more, can't add back

### Tips
- Carpet will stretch and settle — leave a little extra at edges
- Work in sections: driver footwell → center → passenger footwell
- Steering column hole: cut an X pattern, fold flaps under

---

## PART 3: FINISH & INSTALL LRB DASH

**Haynes Ch. 11, Sec. 26 | FSM Group 23 — Instrument Panel**

### Pre-Install
1. Complete any remaining rivets on the LRB assembly
2. Test-fit on the truck BEFORE final assembly if possible
3. Verify all factory mounting points align
4. Confirm steering column clearance (auto column + shifter)

### Installation Sequence
*(Reverse of removal — Haynes Ch. 11, Sec. 26)*

1. Position dash assembly — get help if needed, aluminum is awkward not heavy (~18 lbs)
2. Align to factory mounting points
3. Start all mounting fasteners finger-tight before torquing any
4. **Top screws along windshield** — 8mm, start here for alignment
5. Work outward from center
6. Route any wiring BEFORE final tightening — you need:
   - Instrument cluster connector access
   - HVAC control connections
   - Headlight switch wiring
   - Any accessory wiring
7. Verify column clearance with shifter movement (PRND full sweep)
8. **LRB-specific:** Top cap and lower column panel are removable for future wiring access — don't permanently fasten these

### Reconnect Electrical
- Instrument cluster connector(s)
- HVAC control — electrical + heater cable (red push tab) + vacuum lines
- Headlight switch
- Any other dash switches
- Antenna wire (if routed through dash)

### Post-Install Checks
- [ ] All mounting points secure
- [ ] No rattles when tapped
- [ ] Column clears dash through full tilt range
- [ ] Shifter moves freely PRND without contact
- [ ] All electrical reconnected

---

## PART 4: CLOCKSPRING INSTALLATION

**Haynes Ch. 12, Sec. 21 | FSM Group 8U**

**⚠️ BATTERIES MUST BE DISCONNECTED**

### Centering Procedure (Critical — Get This Right)
1. Front wheels pointing **STRAIGHT AHEAD**
2. Locate locking tabs on new clockspring
3. Depress **BOTH** tabs — hold throughout centering
4. Rotate **CLOCKWISE** until it stops (end of travel)
5. Now count **COUNTERCLOCKWISE:**
   - First full rotation → "one"
   - Second full rotation → "two"
   - Half rotation → "half"
   - **STOP at exactly 2.5 turns**
6. **Verify:** Yellow timing mark visible in window
7. **Verify:** Horn connector at 12 o'clock position
8. If mark NOT visible → start over from step 4

### Install on Column
1. Position clockspring on steering column
2. Snap into place — locating fingers engage detents
3. Route wire harnesses
4. Plug in clockspring harness connector

### Note
Clockspring is for future cruise control. Horn wire will connect through it to the Forever Sharp wheel. Cruise switches need relocation — that's a later task.

---

## PART 5: SEAT INSTALLATION

**Haynes Ch. 11, Sec. 27 | FSM Group 23 — Seats**

### Driver's Seat
1. Position seat over mounting studs
2. Connect wiring harnesses BEFORE bolting down (easier to reach)
3. Install all 4 mounting bolts finger-tight first
4. Torque in sequence:
   - **Front inner → Front outer → Rear inner → Rear outer**
   - **Torque: 35 ft-lbs** *(verified — see FSM torque reference 07)*
5. Snap plastic bolt trim covers back on

### Passenger's Seat
1. Position seat, connect wiring
2. Reconnect under-seat air duct
3. Install mounting bolts finger-tight
4. Torque in sequence:
   - **Front inner → Right outer → Rear bolts**
   - **Torque: 35 ft-lbs**
5. If center seat: install its bolts in the passenger sequence
6. Replace jack cover (front clip, then rear hook)

### Seat Belt Anchors
- **Torque: Check FSM** — typically 30-35 ft-lbs for floor anchors
- Use Haynes Ch. 11, Sec. 21 for seat belt anchor torque
- **These are safety-critical — verify before torquing**

---

## PARTS INVENTORY UPDATE

### From eBay Orders (Delivered Jun 2025)
| Item | Part # | Cost | Status |
|------|--------|------|--------|
| Simpson 260-8P Multimeter | — | $91.96 | ✅ On hand |
| Pitman Arm Nut + Lock Washer (Stainless) | J3200501 | $19.47 | ✅ On hand |
| CR/SKF Radial Shaft Seal | 12411 | $14.06 | ✅ On hand |

**Note on SKF 12411 seal:** 1.250" shaft, 1.979" OD, 0.406" wide — single lip grease seal. Verify application before install. May be for steering box input shaft or transfer case.

**Note on J3200501 nut:** This is a stainless castle nut + lock washer for the pitman arm. You also have a NEW castle nut in the consumables order — use whichever is correct thread/spec for the RedHead sector shaft. **Do not mix — verify thread pitch matches.**

---

## TORQUE QUICK REFERENCE (Today's Tasks Only)

| Component | Torque | Source |
|-----------|--------|--------|
| Seat mounting bolts | 35 ft-lbs | FSM verified |
| Steering wheel nut | 45 ft-lbs | Procedure doc |
| Dash mounting screws | Hand tight / snug | LRB instructions |

---

## CORRECTIONS LOG

| Item | Was | Is | Source |
|------|-----|-----|--------|
| Illumination fuse | 10A | **5A** | FSM fuse chart, fuse #13 (1996-1997) |

---

## NOTES

**Not doing today:**
- Headliner (painting plastics first, routing wires later)
- A-pillar covers / kick panels (same reason)
- Steering wheel install (needs Borgeson shaft connected to box first)
- Seat belts (can do separately)

**Steering wheel sequence (future day):**
1. RedHead box on frame
2. Borgeson shaft: column → box
3. Borgeson pump + hoses
4. Forever Sharp wheel + A17 adapter on column
5. Fill + bleed PS system
6. Test drive

---

*Generated by Akron — 2026-02-06*
*Print this. Take it to the garage. Ask me if you get stuck.*
