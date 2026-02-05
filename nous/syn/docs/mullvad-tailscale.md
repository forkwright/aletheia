# Mullvad VPN + Tailscale Coexistence on Linux

## Problem

When enabling Mullvad VPN on Fedora (and other Linux distributions), it breaks Tailscale connectivity. This happens because:

1. **Aggressive firewall rules**: Mullvad sets strict firewall rules to route all traffic through its VPN tunnel
2. **Routing conflicts**: Mullvad's routing table takes precedence over Tailscale's mesh networking
3. **CGNAT range overlap**: Both services use IP ranges in the 100.x.x.x space, though different subnets

## Understanding the IP Ranges

- **Tailscale**: Uses `100.64.0.0/10` and `fd7a:115c:a1e0::/48` (IPv6)
- **Mullvad**: Routes all traffic through its tunnel interface
- **Conflict**: Mullvad doesn't recognize Tailscale IPs as "local" traffic that should bypass the VPN

## Solution Overview

The solution involves using **traffic marking** with nftables to tell Mullvad's firewall to exclude Tailscale traffic from VPN routing. This approach:

- Marks Tailscale traffic with special identifiers
- Allows marked traffic to bypass Mullvad's tunnel
- Maintains security for both services
- Works only on Linux (not Android/iOS due to platform VPN limitations)

---

## Solution 1: Manual nftables Configuration (Recommended)

### Prerequisites

```bash
# Install nftables (should be available on Fedora by default)
sudo dnf install nftables

# Verify Tailscale IP range
tailscale status
ip addr show tailscale0
```

### Step 1: Create nftables Configuration

Create the nftables rules file:

```bash
sudo mkdir -p /etc/nftables
sudo nano /etc/nftables/mullvad-tailscale.conf
```

Add the following configuration:

```nftables
#!/usr/sbin/nft -f

table inet mullvad_tailscale {
    chain output {
        type route hook output priority -100; policy accept;
        # IPv4 Tailscale traffic
        ip daddr 100.64.0.0/10 ct mark set 0x00000f41 meta mark set 0x6d6f6c65;
    }

    chain input {
        type filter hook input priority -100; policy accept;
        # IPv4 Tailscale traffic (for bidirectional connectivity)
        ip saddr 100.64.0.0/10 ct mark set 0x00000f41 meta mark set 0x6d6f6c65;
    }
}
```

**For IPv6 support**, add these rules to both chains:

```nftables
        # IPv6 Tailscale traffic  
        ip6 daddr fd7a:115c:a1e0::/48 ct mark set 0x00000f41 meta mark set 0x6d6f6c65;
        ip6 saddr fd7a:115c:a1e0::/48 ct mark set 0x00000f41 meta mark set 0x6d6f6c65;
```

### Step 2: Load the Rules

```bash
# Load the rules manually
sudo nft -f /etc/nftables/mullvad-tailscale.conf

# Verify the rules are loaded
sudo nft list ruleset | grep -A 10 mullvad_tailscale
```

### Step 3: Make Persistent (Auto-load on Boot)

Edit the main nftables configuration:

```bash
sudo nano /etc/nftables.conf
```

Add this line to include your configuration:

```bash
include "/etc/nftables/mullvad-tailscale.conf"
```

Enable nftables service:

```bash
sudo systemctl enable nftables
sudo systemctl start nftables
```

### Step 4: Test the Configuration

1. **Connect to Mullvad**: Use the Mullvad app to connect to a server
2. **Test Mullvad**: Verify VPN is working
   ```bash
   curl https://am.i.mullvad.net/connected
   # Should return: "You are connected to Mullvad..."
   ```
3. **Test Tailscale**: Check connectivity to your Tailnet
   ```bash
   tailscale status
   ping $(tailscale ip -4 | head -1)  # Ping your own Tailscale IP
   ping 100.64.0.x  # Replace with another device's Tailscale IP
   ```

---

## Solution 2: Automated Script (Alternative)

For a more automated approach, you can use the community-maintained script:

### Installation

```bash
cd ~/Downloads
git clone https://github.com/r3nor/mullvad-tailscale.git
cd mullvad-tailscale
chmod +x mnf
```

### Configuration

Edit the script configuration:

```bash
nano mnf
```

Modify these variables:
- `RULES_DIR`: Point to the cloned repository directory
- `EXCLUDE_COUNTRY_CODES`: Countries to avoid (optional)

Edit the rules file:

```bash
nano mullvad.rules
```

Set your Tailscale network details:
- `EXCLUDED_IPS`: Your Tailscale device IPs
- `RESOLVER_ADDRS`: Set to `100.100.100.100` for Tailscale DNS

### Usage

```bash
# Connect to Mullvad and apply Tailscale rules
./mnf up

# Connect to a specific country with RAM-only servers
./mnf up --ram --country us

# Disconnect and cleanup
./mnf down

# Apply only the firewall rules (no Mullvad connection)
./mnf conf
```

---

## Solution 3: Systemd Integration (Advanced)

For automatic rule application when Tailscale starts:

### Create nftables Rule Files

```bash
sudo mkdir -p /opt/nftables
```

Create `/opt/nftables/mullvad-tailscale.nft`:

```nftables
table inet mullvad_tailscale {
    chain output {
        type route hook output priority -100; policy accept;
        ip daddr 100.64.0.0/10 ct mark set 0x00000f41 meta mark set 0x6d6f6c65;
    }
    chain input {
        type filter hook input priority -100; policy accept;
        ip saddr 100.64.0.0/10 ct mark set 0x00000f41 meta mark set 0x6d6f6c65;
    }
}
```

Create cleanup file `/opt/nftables/mullvad-tailscale-cleanup.nft`:

```nftables
delete table inet mullvad_tailscale
```

### Modify Tailscale Service

```bash
sudo systemctl edit tailscaled
```

Add the following override:

```ini
[Service]
ExecStartPre=nft -f '/opt/nftables/mullvad-tailscale.nft'
ExecStopPost=nft -f '/opt/nftables/mullvad-tailscale-cleanup.nft'
```

Restart Tailscale:

```bash
sudo systemctl restart tailscaled
```

---

## Alternative: Tailscale's Official Mullvad Integration

**Note**: This only works with Tailscale's hosted service, not self-hosted Headscale.

If you're willing to use Tailscale's cloud service instead of self-hosting:

1. **Enable Mullvad Exit Nodes**:
   - Go to [Tailscale Admin Console](https://login.tailscale.com/admin/machines)
   - Navigate to Access Controls → Exit Nodes
   - Enable Mullvad integration

2. **Connect Your Mullvad Account**:
   - Follow Tailscale's [Mullvad exit nodes guide](https://tailscale.com/kb/1258/mullvad-exit-nodes)
   - Mullvad servers appear as exit nodes in your Tailscale network

3. **Use Mullvad via Tailscale**:
   ```bash
   # List available Mullvad exit nodes
   tailscale exit-node list
   
   # Connect to a Mullvad server via Tailscale
   tailscale set --exit-node=mullvad-us1
   ```

---

## Troubleshooting

### 1. Rules Not Working

**Check if rules are loaded**:
```bash
sudo nft list ruleset | grep mullvad_tailscale
```

**Verify Mullvad is connected**:
```bash
curl https://am.i.mullvad.net/connected
```

**Check Tailscale status**:
```bash
tailscale status
systemctl status tailscaled
```

### 2. Traffic Still Blocked

**Verify marks are being applied**:
```bash
# Monitor packets (requires root)
sudo nft monitor
```

**Check for conflicting rules**:
```bash
sudo nft list ruleset
```

**Ensure priority is correct** (must be between -200 and 0):
```bash
# Priority -100 should work in most cases
# Lower numbers = higher priority
```

### 3. Mullvad App Issues

**Reset Mullvad settings**:
```bash
mullvad disconnect
mullvad auto-connect set off
mullvad auto-connect set on
mullvad connect
```

**Clear firewall rules**:
```bash
sudo nft delete table inet mullvad_tailscale
# Then re-apply the rules
```

### 4. IPv6 Issues

If you experience IPv6 connectivity problems, add IPv6 rules or disable IPv6:

```bash
# Disable IPv6 in Mullvad (if needed)
mullvad tunnel ipv6 set off

# Or add IPv6 rules to your nftables configuration
ip6 daddr fd7a:115c:a1e0::/48 ct mark set 0x00000f41 meta mark set 0x6d6f6c65;
ip6 saddr fd7a:115c:a1e0::/48 ct mark set 0x00000f41 meta mark set 0x6d6f6c65;
```

### 5. DNS Issues

**If DNS resolution breaks**:

1. **Check Tailscale DNS**:
   ```bash
   tailscale status --web  # Check if MagicDNS is enabled
   ```

2. **Verify Mullvad DNS**:
   ```bash
   mullvad dns get
   # Should show your configured DNS servers
   ```

3. **Test DNS resolution**:
   ```bash
   nslookup google.com
   dig @100.100.100.100 your-device-name.tail-scale.ts.net
   ```

---

## Security Considerations

1. **Traffic Inspection**: Tailscale traffic bypasses Mullvad, so it's not routed through Mullvad's servers. However, Tailscale uses WireGuard encryption, so traffic is still encrypted end-to-end.

2. **IP Leakage**: The nftables rules are designed to prevent IP leakage. Traffic is properly marked and routed through appropriate interfaces.

3. **DNS Leaks**: Be aware of DNS configuration. Ensure Tailscale DNS queries go to Tailscale servers (100.100.100.100) and other queries go through Mullvad.

4. **Local Network Access**: The configuration maintains access to your Tailscale network while protecting your internet traffic through Mullvad.

---

## Quick Reference Commands

```bash
# Apply rules manually
sudo nft -f /etc/nftables/mullvad-tailscale.conf

# Remove rules manually  
sudo nft delete table inet mullvad_tailscale

# Check Mullvad connection
curl https://am.i.mullvad.net/connected

# Check Tailscale status
tailscale status

# Test Tailscale connectivity
ping $(tailscale ip -4 | head -1)

# View all nftables rules
sudo nft list ruleset

# Monitor nftables activity
sudo nft monitor
```

---

## References

- [Tailscale Official Documentation: Using with Other VPNs](https://tailscale.com/kb/1105/other-vpns)
- [Mullvad Split Tunneling Guide](https://mullvad.net/en/help/split-tunneling-with-linux-advanced)
- [TheOrangeOne Blog: Accessing Tailscale whilst using Mullvad](https://theorangeone.net/posts/tailscale-mullvad/)
- [GitHub: r3nor/mullvad-tailscale (Automated Script)](https://github.com/r3nor/mullvad-tailscale)
- [GitHub: shervinsahba/mullvad-tailscale-nft (Simple Rules)](https://github.com/shervinsahba/mullvad-tailscale-nft)

---

*Last Updated: January 29, 2026*
*Tested on: Fedora 40/41 with Mullvad App and Tailscale*