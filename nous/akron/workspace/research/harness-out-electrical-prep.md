# Electrical Prep While Harness Is Out
*Created: 2026-02-06*
*Status: Research complete — execute during harness rewrap*

## Context
Dash harness fully unwrapped on tailgate, dash frame bare. Once-in-build access opportunity.

---

## 30-Minute Prep List (Do Before Rewrap)

### 1. Firewall Grommet — Cable Highway
- Driver side, up and left of brake pedal, near A-pillar
- Pull grommet out, punch/drill for 1" split loom conduit
- Feed split loom through as permanent cable highway
- Seal with RTV silicone after all cables through
- **Everything below routes through this one point**

### 2. Pre-Route Gauge Wires to A-Pillar
- 3 labeled wire pairs (14-16 AWG marine) for EGT, Boost, Trans Temp
- Route from firewall grommet area → along main harness path → up toward A-pillar
- Leave 18" coiled slack at each end
- Wrap into main harness bundle with X-Fasten tape
- Label both ends clearly
- **EGT thermocouple extension wire** also routes through firewall grommet
- Note: A-pillar pod requires cutting holes in original pillar cover behind pod location

### 3. Pre-Route Audio Signal Path (RCA)
- Sony XAV-9000ES → JL Audio RD500/1
- **CRITICAL: RCA signal on PASSENGER side, power wire on DRIVER side** (prevents alternator whine)
- Run pull-string or actual RCA cables from center dash (head unit) → passenger side kick panel → amp location (under/behind seat TBD)
- Remote turn-on wire (blue/white from Sony) runs with RCAs
- JL amp power wire (4 AWG) goes through driver-side firewall grommet — don't run wire yet, just confirm path

### 4. Add AUX GND Point on Dash Frame
- One dedicated ground for future accessories (gauges, radio head, etc.)
- 10 AWG marine tinned wire + ring terminal
- Bolt to dash frame with star washer on clean bare metal
- Label "AUX GND"
- Separate from factory grounds — low-current dash accessories only

### 5. Yaesu FTM-510DR Prep
- Separable head unit — body mounts hidden, head on dash
- Body needs DIRECT battery power (not fuse box) — 10 AWG minimum for 50W TX
- Route separation cable path from body location to head unit location
- Antenna cable path: from radio body → through dash → toward roofline (leave labeled pull-string for when headliner goes in)
- Power cable routes through firewall grommet with amp power

### 6. Garmin PowerSwitch
- Mounts ENGINE BAY near battery (Garmin spec: must be close to battery)
- System 2: light bars, rock lights, compressor — all exterior
- Only the control cable to Garmin Tread 2 comes back into cab
- Pre-route: control cable path from dash to firewall grommet

---

## What NOT To Do Now

| Skip | Why |
|---|---|
| Run actual 4 AWG amp power wire | Confirm amp location first |
| Backup camera cable | Goes through body, not dash harness |
| 6-gang rocker panel wiring | House power (System 3), not dash-routed |
| Mechman/Big 3 wiring | All engine bay |
| Aux power system routing | Blocked by alternator. Don't pre-route what might change. |

---

## Blue Sea 5032 Location
- For System 3 (house power) — LiFePO4, inverter, DC-DC feeds
- Mount where house battery goes (under rear seat or bed/toolbox)
- **NOT behind the dash** — no dash routing needed

---

## Equipment Reference
| Item | Power Req | Signal | Mount Location |
|---|---|---|---|
| Sony XAV-9000ES | Dash fuse box (ACC) | RCA out to amp | Center dash (Metra kit) |
| JL Audio RD500/1 | Direct battery 4AWG + 50A fuse | RCA in from Sony | Under/behind seat TBD |
| AutoMeter EGT | 12V switched | Thermocouple from exhaust | A-pillar pod |
| AutoMeter Boost | 12V switched | 1/8 NPT tee on intake | A-pillar pod |
| AutoMeter Trans Temp | 12V switched | Sender in trans pan | A-pillar pod |
| Yaesu FTM-510DR | Direct battery 10AWG | Antenna coax (NMO) | Body hidden, head on dash |
| Garmin PowerSwitch | Direct battery (supplied cable) | BLE to Tread 2 + control wire | Engine bay near battery |
| Garmin Tread 2 | USB/12V from PowerSwitch or ACC | — | Dash mount |
| Getac K120 | 12V or own battery | — | RAM mount TBD |
| NATIKA backup camera | 12V reverse trigger | Composite video | Rear |

---

*Sources: Garmin PowerSwitch manual, JL Audio RD500/1 manual, DodgeForum firewall grommet threads, Overland Equipped aux power guide, DIYMobileAudio RCA routing best practices*
