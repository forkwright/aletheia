#!/usr/bin/env python3
"""
Verification script to check that all akron/ and autarkeia/ files have proper frontmatter and cross-linking.
"""

import os
import re
from pathlib import Path

def check_frontmatter(content):
    """Check if frontmatter is properly formatted."""
    if not content.strip().startswith('---'):
        return False, "No frontmatter"
    
    lines = content.split('\n')
    if len(lines) < 3:
        return False, "Incomplete frontmatter"
        
    # Check for required fields
    has_created = False
    has_tags = False
    
    for line in lines[1:]:
        if line.strip() == '---':
            break
        if line.startswith('created:'):
            has_created = True
        if line.startswith('tags:'):
            has_tags = True
    
    if not has_created:
        return False, "Missing created field"
    if not has_tags:
        return False, "Missing tags field"
        
    return True, "OK"

def check_see_also(content):
    """Check if file has see also section."""
    return "> See also:" in content

def check_file(file_path):
    """Check a single file for completeness."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        frontmatter_ok, frontmatter_msg = check_frontmatter(content)
        has_see_also = check_see_also(content)
        
        return {
            'file': str(file_path),
            'frontmatter_ok': frontmatter_ok,
            'frontmatter_msg': frontmatter_msg,
            'has_see_also': has_see_also,
            'has_wikilinks': '[[' in content
        }
        
    except Exception as e:
        return {
            'file': str(file_path),
            'error': str(e)
        }

def main():
    """Verify all markdown files."""
    base_path = Path("/mnt/ssd/aletheia/theke")
    
    # Get all files
    akron_files = list((base_path / "akron").glob("**/*.md"))
    autarkeia_files = list((base_path / "autarkeia").glob("**/*.md"))
    all_files = akron_files + autarkeia_files
    
    print(f"Checking {len(all_files)} files...\n")
    
    issues = []
    good_files = 0
    
    for file_path in sorted(all_files):
        result = check_file(file_path)
        
        if 'error' in result:
            issues.append(f"ERROR {result['file']}: {result['error']}")
            continue
            
        file_issues = []
        
        if not result['frontmatter_ok']:
            file_issues.append(f"Frontmatter: {result['frontmatter_msg']}")
        
        if not result['has_see_also'] and 'README' in str(file_path):
            file_issues.append("Missing 'See also' section (README file)")
            
        if not result['has_wikilinks']:
            file_issues.append("No wikilinks found")
        
        if file_issues:
            issues.append(f"ISSUES {result['file']}: {'; '.join(file_issues)}")
        else:
            good_files += 1
    
    print(f"✓ {good_files} files properly processed")
    print(f"⚠ {len(issues)} files with issues\n")
    
    if issues:
        print("Issues found:")
        for issue in issues:
            print(f"  {issue}")
    else:
        print("🎉 All files properly processed!")

if __name__ == "__main__":
    main()