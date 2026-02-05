# Complete Radio and Communications System Summary
**Date:** January 2025
**Source:** `/mnt/ssd/moltbot/dianoia/autarkeia/praxis/radio/`

## Executive Overview

This system provides comprehensive communications capability across multiple technologies, bands, and use cases. The setup includes 12 distinct radio systems integrated through power management, antenna systems, and Meshtastic mesh networking. Primary users are Cody and Kendall with vehicle-mounted systems in a 1997 Ram 2500, Corolla Cross, and Royal Enfield GT650 motorcycle.

**Key Capabilities:**
- Local communications (VHF/UHF): 0.5-50 miles
- Mesh networking (Meshtastic): 5-15+ miles with relay capability
- Digital modes (C4FM, APRS)
- Emergency monitoring and cross-band repeat
- Mobile repeater capability via Yaesu FTM-510DR
- SDR spectrum monitoring: 500 kHz - 1.7 GHz

---

## 1. Radio Equipment Inventory

### 1.1 Handheld Radios (4 Units)

#### Primary Handhelds
| System | Radio | Power | Battery | Primary Use |
|--------|-------|-------|---------|-------------|
| System 1 | Baofeng UV-5RM Plus #1 | 10W | 2500mAh | Truck handheld |
| System 2 | Baofeng UV-5RM Plus #2 | 10W | 2500mAh | General portable |
| System 3 | Baofeng BF-F8HP | 8W | 3800mAh USB-C | High-power/emergency |
| System 4 | Baofeng UV-5R | 4W | 1800mAh | Backup/loaner |

**Key Features:**
- All CHIRP-compatible for programming
- 50 channels programmed on primary units
- Dual-band VHF/UHF operation
- Cross-compatible accessories
- System 3 has extended runtime with BTECH BL-5L battery and USB-C charging

### 1.2 Mobile/Base Station Radios

#### System 5: Yaesu FTM-510DR (Truck Mobile)
- **Power:** 50W VHF/UHF output
- **Digital Modes:** C4FM System Fusion, APRS
- **Special Capability:** Cross-band repeat (mobile repeater function)
- **Mount:** Permanent installation in 1997 Ram 2500
- **Power Source:** Hardwired to Renogy 100Ah LiFePO4 auxiliary battery

#### System 9: RAK2245 LoRaWAN Gateway (Home Base)
- **Technology:** LoRaWAN concentrator (915MHz)
- **Range:** 15km+ line-of-sight capability
- **Host:** Raspberry Pi 4
- **GPS:** Integrated ublox MAX-7Q for time sync
- **Purpose:** Meshtastic gateway and LoRaWAN infrastructure

### 1.3 Software Defined Radio

#### System 6: ThinkPad T480 Multi-Mode Station
- **SDR:** RTL-SDR Blog V4 R828D (500 kHz - 1766 MHz)
- **Meshtastic:** RAK19007 + RAK4631 + RAK12500 GNSS
- **Software:** SDR#/GQRX, Meshtastic Python CLI
- **Capability:** Spectrum monitoring, signal analysis, mesh gateway

---

## 2. Meshtastic Mesh Network Setup

### 2.1 Network Architecture

**5-Node Mesh Network:**
| Node | Device | Location | Battery | Antenna |
|------|--------|----------|---------|---------|
| Node 1 | T-Echo #1 | Ram 2500 | 5000mAh external | Stock 915MHz |
| Node 2 | T-Echo #2 | Corolla Cross | 5000mAh external | Stock 915MHz |
| Node 3 | T-Echo #3 | GT650 Motorcycle | 5000mAh external | Stock 915MHz |
| Node 4 | T-Deck Plus (Cody) | Portable | 2000mAh internal | 10dBi/compact switchable |
| Node 5 | T-Deck Plus (Kendall) | Portable | 2000mAh internal | 10dBi/compact switchable |
| Gateway | RAK2245 (Home) | Base station | Grid power | Planned professional mount |

### 2.2 Mesh Network Capabilities

**Communication Types:**
- Off-grid text messaging between all nodes
- GPS position sharing and tracking
- Emergency beacon capability
- Range extension via relay nodes
- Bluetooth integration with smartphones

**Coverage:**
- Vehicle nodes provide mobile relay capability
- T-Deck Plus units serve as primary messaging terminals
- RAK2245 provides wide-area base coverage (15km+)
- Automatic routing through mesh topology

### 2.3 Integration Features

- **Smartphone Integration:** Meshtastic app connects via Bluetooth to T-Deck Plus
- **Briar Mesh:** Close-range encrypted messaging (100m device-to-device)
- **GPS Tracking:** All nodes share position data
- **Power Management:** Vehicle nodes powered continuously, portables on battery

---

## 3. SDR and Digital Equipment

### 3.1 Software Defined Radio

#### RTL-SDR V4 System
- **Frequency Range:** 500 kHz - 1766 MHz
- **Features:** 1PPM TCXO for accuracy, direct sampling
- **Antenna:** ANT500 telescopic (75MHz-1GHz, 20-88cm adjustable)
- **Applications:** 
  - Spectrum monitoring
  - Signal analysis
  - Aircraft tracking (ADS-B)
  - Trunked radio monitoring

### 3.2 Digital Radio Modes

#### Yaesu FTM-510DR Digital Capabilities
- **C4FM System Fusion:** Digital voice with GPS integration
- **APRS:** Automatic position reporting and messaging
- **Cross-band Repeat:** Extends handheld range dramatically

#### LoRaWAN Infrastructure
- **RAK2245 Gateway:** 8 uplink channels, 1 downlink
- **Power:** Up to 27dBm transmit, -139dBm receive sensitivity
- **Integration:** Raspberry Pi based with GPS time synchronization

### 3.3 Emergency Digital Communications

#### Planned Winlink Capability
- Requires HF radio addition (Yaesu FT-891)
- Global radio email system
- Emergency message forwarding
- Computer interface for packet operations

---

## 4. Power Systems for Radio

### 4.1 Vehicle Power (1997 Ram 2500)

#### Primary Power System
- **Battery:** Renogy Core Mini 12.8V 100Ah LiFePO4
- **Charging:** Renogy 40A DC-DC charger with MPPT
- **Load Distribution:**
  - Yaesu FTM-510DR (permanent)
  - T-Echo #1 via 5000mAh buffer battery
  - UV-5RM Plus #1 (12V powered when docked)

#### Planned Solar Enhancement
- Flexible solar panels on cab roof
- Integration with existing Renogy battery system
- Maintain power during extended parking

### 4.2 Portable Power Systems

#### Handheld Radio Batteries
| Radio | Battery Type | Capacity | Special Features |
|-------|-------------|----------|------------------|
| BF-F8HP | BTECH BL-5L | 3800mAh | USB-C charging, 15+ hour standby |
| UV-5RM Plus (x2) | Li-ion | 2500mAh | Standard BL-5 compatible |
| UV-5R | Li-ion | 1800mAh | Standard configuration |

#### Meshtastic Power
- **T-Echo units:** 5000mAh external batteries (vehicle-mounted)
- **T-Deck Plus:** Upgrading to 5000mAh internal batteries
- **Vehicle integration:** Powered continuously via 12V systems

### 4.3 Computer Power

#### ThinkPad T480 Mobile Station
- **Total Capacity:** 140Wh (24Wh internal + 92Wh external + 24Wh spare)
- **Hot-swap capability:** External battery replacement without shutdown
- **Runtime:** 8-12 hours for SDR and Meshtastic operations

### 4.4 Home Power Systems

#### UPS Backup
- All home radio and network equipment on UPS
- RAK2245 gateway protected from power interruptions
- Network infrastructure maintained during outages

---

## 5. Antenna Inventory

### 5.1 VHF/UHF Antennas

#### Handheld Antennas
| Antenna | Frequency | Gain | Length | Radio Assignment |
|---------|-----------|------|---------|------------------|
| Nagoya NA-771R | VHF/UHF | Standard | 16" retractable | UV-5RM Plus #2 |
| RD-771 | VHF/UHF | High gain | Long | UV-5RM Plus #1 (truck) |
| V-85 | Dual band | Standard | Standard | BF-F8HP |
| Stock 8"/15" | VHF/UHF | Standard | 8"/15" | UV-5RM Plus backup |

#### Mobile Antennas
| Antenna | Power Rating | Mount | Application |
|---------|--------------|-------|-------------|
| Tram 1180 | 150W | NMO | Truck - Yaesu FTM-510DR |
| Midland MXTA27 | Mobile | Lip mount | Universal NMO mounting |

### 5.2 Specialized Antennas

#### SDR Antennas
- **ANT500:** Telescopic 75MHz-1GHz, 20-88cm adjustable, SMA male
- **Coverage:** Full RTL-SDR frequency range

#### Meshtastic Antennas
| Antenna | Gain | Length | Use Case |
|---------|------|---------|----------|
| MakerHawk 10dBi | 10dBi | 17cm | Maximum range (T-Deck Plus) |
| GRA-SCH32 | Tri-band | 1.9" | Stealth/portable (T-Deck Plus) |
| Stock 915MHz | Standard | Standard | T-Echo units |

### 5.3 Planned Antenna Infrastructure

#### RAK2245 Base Station Antenna
- **Nagoya UT-72:** Magnetic mount with ground plane
- **Grounding:** 4' copper rod with 8-gauge ground wire
- **Installation:** Home server room with professional RF grounding

---

## 6. Frequency Allocations and Licensing

### 6.1 Frequency Plan (50 Channels Programmed)

#### Family/Tactical (Channels 1-3)
| Channel | Frequency | License Required | CTCSS | Power |
|---------|-----------|------------------|-------|-------|
| 1 | 146.940 MHz | HAM Technician | 88.5 | High |
| 2 | 446.125 MHz | HAM Technician | 88.5 | High |
| 3 | 151.880 MHz | MURS overlap | 88.5 | Low |

#### Amateur Radio (Channels 4-11)
- **Simplex:** 146.520 MHz (2M calling), 446.000 MHz (70cm calling)
- **Repeaters:** 8 local repeaters including AARC System Fusion
- **APRS:** 144.390 MHz for position reporting

#### Emergency Services (Channels 12-17)
- **Monitor Only:** Fire dispatch, mutual aid, search & rescue
- **ARES:** Emergency amateur radio nets

#### GMRS/FRS (Channels 18-39)
- **GMRS:** Channels 1-7, 15-22 (requires license)
- **FRS:** Channels 8-14 (license-free, 0.5W limit)

#### MURS (Channels 40-44)
- **License-free:** 2W maximum, external antennas allowed
- **Business/personal use:** 151.820-154.600 MHz

#### NOAA Weather (Channels 45-50)
- **All 7 weather channels** programmed for Austin area coverage

### 6.2 Current Licensing Status

#### Active Licenses
- **GMRS License:** Active ($35/10 years, covers entire family)

#### Required Licenses
- **Amateur Radio:** Planned - Technician class minimum
  - Required for channels 1-2, 4-11, 16-17
  - Cost: $15 testing fee
  - Local VE sessions monthly

### 6.3 Legal Operating Guidelines

#### No License Required
- FRS channels 8-14 (0.5W maximum)
- MURS channels 1-5 (2W maximum)
- NOAA Weather (receive only)
- Emergency services monitoring (receive only)
- Emergency transmissions during life-threatening situations

#### Licensed Operation
- **GMRS:** 50W maximum on main channels
- **Amateur:** Per band plan rules, requires station identification

---

## 7. System Integration and Interoperability

### 7.1 Radio-to-Radio Integration

#### Cross-Band Repeat Capability
- **Yaesu FTM-510DR** can relay between VHF and UHF
- Extends handheld range from 5 miles to 25+ miles
- **Example:** UV-5RM Plus #1 (truck handheld) → FTM-510DR → distant station

#### Digital Mode Integration
- **C4FM System Fusion:** Digital voice with GPS data
- **APRS:** Position reporting from FTM-510DR to internet
- **Bluetooth:** FTM-510DR can interface with smartphones

### 7.2 Mesh Network Integration

#### Meshtastic Ecosystem
- **5 active nodes** providing mesh coverage
- **Smartphone integration** via Bluetooth to T-Deck Plus units
- **Vehicle-based relays** extend coverage area
- **Base station gateway** provides wide-area coordination

#### Multi-Technology Bridging
- Smartphones bridge cellular to Meshtastic
- SDR system provides monitoring across all bands
- Manual coordination between radio and mesh networks

### 7.3 Power System Integration

#### Vehicle Integration (Ram 2500)
- Single 100Ah LiFePO4 battery powers:
  - Yaesu FTM-510DR (50W mobile radio)
  - T-Echo mesh node (continuous operation)
  - UV-5RM Plus charging when docked
- **Future HF integration:** Same power system will support FT-891

#### Computer Station Integration
- ThinkPad T480 provides:
  - SDR monitoring and analysis
  - Meshtastic gateway functions
  - CHIRP programming for all radios
  - Future Winlink digital email capability

---

## 8. Gaps and Planned Additions

### 8.1 High Priority Additions

#### HF Capability
**Yaesu FT-891 - HF/6M Mobile Transceiver**
- **Capability:** Long-range communications (1000+ miles)
- **Mounting:** 1997 Ram 2500 (companion to FTM-510DR)
- **Power:** 100W, connects to existing Renogy 100Ah system
- **Antennas:** ATAS-120A auto-tuning or hamstick options
- **Cost:** $650-700
- **Timeline:** Next major purchase

#### Amateur Radio Licensing
- **Minimum:** Technician class for VHF/UHF repeaters
- **Preferred:** General class for HF privileges
- **Cost:** $15 testing fee
- **Timeline:** Next 3-6 months

### 8.2 Medium Priority Enhancements

#### Solar Power System (Vehicle)
- **Components:** Flexible panels for truck cab roof
- **Integration:** Connect to existing Renogy system
- **Benefit:** Maintain auxiliary battery during extended parking
- **Cost:** $300-500

#### GMRS Repeater
- **Retevis RT97:** Portable repeater
- **Capability:** Extend GMRS range to 20+ miles
- **Use:** Deploy for events or emergencies
- **Cost:** ~$400

#### Professional Base Station Antenna
- **Nagoya UT-72 + grounding system** (already purchased)
- **Installation:** Home server room
- **Benefit:** Improved performance for RAK2245 gateway

### 8.3 Long-Term Expansion

#### Winlink Gateway Capability
- **Requirement:** HF radio (FT-891) + computer interface
- **Capability:** Radio email system with global message forwarding
- **Timeline:** After HF radio installation

#### Additional Meshtastic Infrastructure
- Fixed solar-powered nodes for property coverage
- T-Beam units (higher power than T-Echo)
- LoRaWAN sensor network integration

#### QRP Portable Operations
- **Xiegu G90 or Icom IC-705:** Portable HF radio
- **Antennas:** EFHW wire, Buddipole system
- **Use:** Field Day, emergency deployment, portable operations

### 8.4 Current Limitations

#### Range Limitations
- **VHF/UHF handhelds:** 5-15 miles depending on terrain
- **Mesh network:** Limited by node density and terrain
- **No HF capability:** Missing long-range communications
- **Antenna height:** Ground-level antennas limit range

#### Power Limitations
- **Portable operations:** Limited by battery capacity
- **No solar charging:** Vehicle systems require alternator operation
- **Home backup:** UPS provides limited runtime

#### Licensing Constraints
- **Amateur frequencies:** Cannot legally use without license
- **Power limitations:** FRS limited to 0.5W on some channels
- **Identification requirements:** GMRS requires call sign

---

## System Statistics

### Equipment Summary
- **Total Radio Systems:** 12 distinct systems
- **Active Radio Units:** 10 operational radios
- **Meshtastic Nodes:** 5 active nodes + 1 gateway
- **Frequency Coverage:** 136 MHz - 1.7 GHz
- **Programmed Channels:** 50 channels across all services
- **Documentation Files:** 66 files (manuals, specs, references)

### Coverage Capabilities
- **Local Communications:** 0.5-50 miles (depending on radio and power)
- **Mesh Network:** 5-15+ miles with relay capability
- **Spectrum Monitoring:** 500 kHz - 1.7 GHz
- **Digital Modes:** C4FM, APRS, LoRaWAN, Meshtastic
- **Emergency Monitoring:** Fire, EMS, SAR, NOAA Weather

### Power System Totals
- **Vehicle Battery:** 100Ah LiFePO4 (1280Wh)
- **Handheld Batteries:** 12.8Ah total capacity across all units
- **Computer Power:** 140Wh hot-swappable system
- **Mesh Node Power:** 30Ah total portable battery capacity

---

## Conclusion

This communications system provides comprehensive coverage from local handheld operations to wide-area mesh networking, with planned expansion into HF long-range communications. The integrated power systems, standardized programming, and multi-technology approach create a resilient communication capability suitable for daily use, emergency response, and extended off-grid operations.

The system's strength lies in its redundancy, integration, and scalability. Multiple technologies provide backup communication paths, vehicle integration ensures mobile capability, and the mesh network offers off-grid coordination. Planned additions will address current gaps in HF capability and licensing, creating a complete amateur radio station with emergency communication capabilities.

---

*This summary is based on documentation at `/mnt/ssd/moltbot/dianoia/autarkeia/praxis/radio/` as of January 2025.*