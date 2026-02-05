#!/bin/bash
# Filesystem Migration Script - Phase 3
# Run this BEFORE config changes (which require restart)

set -e
cd /mnt/ssd/aletheia

echo "=== Phase 3A: Create Structure ==="
mkdir -p projects
mkdir -p agents
mkdir -p infrastructure

echo "=== Phase 3B: Move Projects from dianoia ==="

# Vehicle (15GB)
echo "Moving vehicle..."
mv dianoia/autarkeia/praxis/vehicle projects/vehicle

# Craft (2.5GB) - Demi's domain
echo "Moving craft (poiesis)..."
mv dianoia/poiesis projects/craft

# Career & Personal
echo "Moving career, personal, reference..."
mv dianoia/autarkeia/career projects/career
mv dianoia/autarkeia/episteme projects/reference
mv dianoia/autarkeia/personal projects/personal
mv dianoia/autarkeia/personal_portfolio projects/portfolio
mv dianoia/autarkeia/immigrate projects/immigrate

# Praxis subdirs
echo "Moving praxis subdirs..."
mv dianoia/autarkeia/praxis/preparedness projects/preparedness
mv dianoia/autarkeia/praxis/radio projects/radio
mv dianoia/autarkeia/praxis/family projects/family

# Smaller project dirs
echo "Moving smaller projects..."
mv dianoia/metaxynoesis projects/metaxynoesis
mv dianoia/energeia projects/energeia
mv dianoia/apotelesma projects/outputs
mv dianoia/inbox projects/inbox

echo "=== Phase 3C: Move Projects from clawd ==="
echo "Moving mba and work..."
mv clawd/mba projects/mba
mv clawd/work projects/work

echo "=== Phase 3D: Move Agents ==="
echo "Moving agent workspaces..."
mv chiron agents/
mv eiron agents/
mv demiurge agents/
mv syl agents/
mv arbor agents/
mv akron agents/

# THE BIG ONE - rename clawd → syn
echo "Renaming clawd → agents/syn..."
mv clawd agents/syn

echo "=== Phase 3E: Infrastructure ==="
echo "Moving infrastructure..."
mv repos infrastructure/
mv signal-cli infrastructure/
mv data infrastructure/

echo "=== Phase 3F: Archive Dianoia Structure ==="
mkdir -p archive/dianoia-structure
cp dianoia/CLAUDE.md archive/dianoia-structure/ 2>/dev/null || true
cp dianoia/llms.txt archive/dianoia-structure/ 2>/dev/null || true
cp dianoia/naming_system.md archive/dianoia-structure/ 2>/dev/null || true
cp dianoia/README.md archive/dianoia-structure/ 2>/dev/null || true
cp dianoia/CHANGELOG.md archive/dianoia-structure/ 2>/dev/null || true

# Clean up remaining dianoia dirs
echo "Cleaning up remaining dianoia..."
rm -rf dianoia/autarkeia  # Should be empty now
rm -rf dianoia/context
rm -rf dianoia

echo "=== Phase 3G: Create Symlinks ==="
# Backwards compatibility
ln -s agents/syn clawd
ln -s projects craft  # For Demi's muscle memory

echo "=== Phase 3 Complete ==="
echo ""
echo "New structure:"
ls -la /mnt/ssd/aletheia/
echo ""
echo "Projects:"
ls -la /mnt/ssd/aletheia/projects/
echo ""
echo "Agents:"
ls -la /mnt/ssd/aletheia/agents/

echo ""
echo "⚠️  NEXT STEP: Apply config changes and restart OpenClaw"
echo "Run: openclaw gateway config.patch (with the new config)"
