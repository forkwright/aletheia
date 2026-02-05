#!/bin/bash
# Capstone sync infrastructure

CAPSTONE_LOCAL="/mnt/ssd/aletheia/clawd/mba/sp26/capstone"
DRIVE_ROOT="gdrive-school:TEMBA/SP26 Capstone"

echo "=== Capstone Sync Status ==="
echo "Local path: $CAPSTONE_LOCAL"
echo "Drive path: $DRIVE_ROOT"

# Check if Drive folder exists
echo "Checking Drive folder..."
if ! rclone lsd "$DRIVE_ROOT" > /dev/null 2>&1; then
    echo "❌ Drive folder doesn't exist yet"
    echo "📁 Need to create: $DRIVE_ROOT"
else
    echo "✅ Drive folder exists"
fi

# Show local structure
echo "Local structure:"
find "$CAPSTONE_LOCAL" -name "*.md" -o -name "*.docx" | head -10

# Show any Drive content
echo "Drive content:"
rclone ls "$DRIVE_ROOT" 2>/dev/null | head -10 || echo "No content found"

echo "=== End Status ==="