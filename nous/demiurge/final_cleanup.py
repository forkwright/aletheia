#!/usr/bin/env python3
"""
Final cleanup script to fix remaining frontmatter and cross-linking issues.
"""

import os
import re
from datetime import datetime
from pathlib import Path

def fix_incomplete_frontmatter(file_path):
    """Fix frontmatter missing 'created' field."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        if not content.strip().startswith('---'):
            return False, "No frontmatter at all"
        
        lines = content.split('\n')
        frontmatter_end = -1
        has_created = False
        
        for i, line in enumerate(lines[1:], 1):
            if line.strip() == '---':
                frontmatter_end = i
                break
            if line.startswith('created:'):
                has_created = True
        
        if has_created:
            return False, "Already has created field"
        
        if frontmatter_end == -1:
            return False, "No frontmatter end found"
        
        # Add created field after the first --- line
        created_date = '2026-02-10'  # Use a standard date for cleanup
        lines.insert(1, f'created: {created_date}')
        
        new_content = '\n'.join(lines)
        
        with open(file_path, 'w', encoding='utf-8') as f:
            f.write(new_content)
        
        return True, "Added created field"
        
    except Exception as e:
        return False, f"Error: {e}"

def add_see_also_to_readme(file_path):
    """Add See also section to README files."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        if "> See also:" in content:
            return False, "Already has See also"
        
        lines = content.split('\n')
        new_lines = []
        title_found = False
        
        for line in lines:
            new_lines.append(line)
            if line.startswith('# ') and not title_found:
                # Add See also after title
                path_str = str(file_path)
                see_also_links = []
                
                if 'akron' in path_str:
                    see_also_links.append('[[akron/README|vehicle systems]]')
                    if 'royal_enfield' in path_str:
                        see_also_links.append('[[dodge_ram_2500_1997/documentation/README|truck documentation]]')
                    elif 'scripts' in path_str:
                        see_also_links.append('[[database/README|database tools]]')
                
                if 'autarkeia' in path_str:
                    see_also_links.append('[[autarkeia/README|preparedness systems]]')
                    if 'civil-rights' in path_str:
                        see_also_links.append('[[firearms/README|firearms]], [[radio/README|communications]]')
                
                if see_also_links:
                    new_lines.append('')
                    new_lines.append('> See also: ' + ', '.join(see_also_links))
                    new_lines.append('')
                
                title_found = True
        
        if title_found:
            new_content = '\n'.join(new_lines)
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(new_content)
            return True, "Added See also section"
        else:
            return False, "No title found"
        
    except Exception as e:
        return False, f"Error: {e}"

def add_basic_wikilinks(file_path):
    """Add basic wikilinks to files that don't have any."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        if '[[' in content:
            return False, "Already has wikilinks"
        
        # Don't modify archived or processed manual files
        if '/archive/' in str(file_path) or '/processed/' in str(file_path):
            return False, "Skipping archive/processed file"
        
        # Add a basic cross-reference after the title if it doesn't exist
        lines = content.split('\n')
        new_lines = []
        title_found = False
        
        for line in lines:
            new_lines.append(line)
            if line.startswith('# ') and not title_found and '> See also:' not in content:
                # Add a basic cross-reference
                path_str = str(file_path).lower()
                
                if 'akron' in path_str and 'procedure' in path_str:
                    new_lines.append('')
                    new_lines.append('> See also: [[00_master-vehicle-record|master vehicle record]], [[02_maintenance-service|maintenance log]]')
                    new_lines.append('')
                elif 'akron' in path_str and 'reference' in path_str:
                    new_lines.append('')
                    new_lines.append('> See also: [[00_master-vehicle-record|master vehicle record]], [[akron/README|vehicle systems]]')
                    new_lines.append('')
                elif 'autarkeia' in path_str:
                    new_lines.append('')
                    new_lines.append('> See also: [[autarkeia/README|preparedness systems]]')
                    new_lines.append('')
                
                title_found = True
        
        if title_found:
            new_content = '\n'.join(new_lines)
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(new_content)
            return True, "Added basic wikilinks"
        else:
            return False, "No title found or no changes needed"
        
    except Exception as e:
        return False, f"Error: {e}"

def main():
    """Apply final cleanup to all files."""
    base_path = Path("/mnt/ssd/aletheia/theke")
    
    # Get all files
    akron_files = list((base_path / "akron").glob("**/*.md"))
    autarkeia_files = list((base_path / "autarkeia").glob("**/*.md"))
    all_files = akron_files + autarkeia_files
    
    print(f"Applying final cleanup to {len(all_files)} files...\n")
    
    fixed_frontmatter = 0
    added_see_also = 0
    added_wikilinks = 0
    
    for file_path in sorted(all_files):
        # Fix incomplete frontmatter
        success, msg = fix_incomplete_frontmatter(file_path)
        if success:
            fixed_frontmatter += 1
            print(f"Fixed frontmatter: {file_path}")
        
        # Add See also to README files
        if 'README' in str(file_path):
            success, msg = add_see_also_to_readme(file_path)
            if success:
                added_see_also += 1
                print(f"Added See also: {file_path}")
        
        # Add basic wikilinks
        success, msg = add_basic_wikilinks(file_path)
        if success:
            added_wikilinks += 1
            print(f"Added wikilinks: {file_path}")
    
    print(f"\nFinal cleanup complete:")
    print(f"  Fixed frontmatter: {fixed_frontmatter} files")
    print(f"  Added See also: {added_see_also} files")  
    print(f"  Added wikilinks: {added_wikilinks} files")

if __name__ == "__main__":
    main()