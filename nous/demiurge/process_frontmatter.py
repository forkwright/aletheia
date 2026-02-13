#!/usr/bin/env python3
"""
Script to add frontmatter, tags, and cross-links to akron/ and autarkeia/ markdown files.
"""

import os
import re
from datetime import datetime
from pathlib import Path

def determine_tags(file_path, content):
    """Determine appropriate tags based on file path and content."""
    path_str = str(file_path).lower()
    content_lower = content.lower()
    
    tags = []
    
    # Determine type tag
    if 'readme' in path_str or 'index' in path_str:
        tags.append('index')
    elif any(x in path_str for x in ['procedure', 'install', 'maintenance_cycle', 'checklist']):
        tags.append('procedure')
    elif any(x in path_str for x in ['specification', 'inventory', 'torque', 'reference', 'guide']):
        tags.append('reference')
    elif any(x in path_str for x in ['log', 'history', 'record']):
        tags.append('log')
    elif any(x in path_str for x in ['project', 'build', 'plan', 'analysis', 'gap']):
        tags.append('project')
    elif any(x in content_lower for x in ['analysis', 'assessment', 'tracking']):
        tags.append('analysis')
    else:
        tags.append('reference')  # default
    
    # Determine topic tags
    if 'akron' in path_str or any(x in path_str for x in ['dodge', 'ram', 'enfield', 'vehicle']):
        tags.append('vehicle')
    
    if 'autarkeia' in path_str or any(x in path_str for x in ['preparedness', 'emergency', 'firearms', 'radio']):
        tags.append('preparedness')
        
    if any(x in path_str for x in ['teardrop', 'overland']):
        tags.append('preparedness')
        
    if 'audio' in path_str:
        tags.append('audio')
        
    return tags

def generate_aliases(file_path, title):
    """Generate aliases based on filename and title."""
    filename = Path(file_path).stem
    aliases = []
    
    # Add shortened filename
    if filename != title.lower().replace(' ', '-'):
        aliases.append(filename.replace('_', ' ').replace('-', ' '))
    
    # Add key terms from title
    title_words = title.lower().split()
    if len(title_words) > 3:
        # Take first and last words, or key terms
        key_terms = [title_words[0], title_words[-1]]
        aliases.append(' '.join(key_terms))
    
    return list(set(aliases))  # Remove duplicates

def generate_see_also_links(file_path, content):
    """Generate cross-reference links based on file location and content."""
    path_str = str(file_path)
    links = []
    
    # Akron domain links
    if 'akron' in path_str:
        if 'dodge_ram' in path_str:
            if not '00_master-vehicle-record' in path_str:
                links.append('[[00_master-vehicle-record|master vehicle record]]')
        if 'teardrop' in path_str or 'overland' in path_str:
            links.append('[[autarkeia/README|preparedness]]')
        if 'audio' in path_str.lower():
            links.append('[[poiesis/README|audio documentation]]')
            
    # Autarkeia domain links  
    if 'autarkeia' in path_str:
        if 'emergency' in path_str or 'evacuation' in path_str:
            links.append('[[oikia/README|household planning]]')
        if 'overland' in path_str:
            links.append('[[akron/overland_teardrop/README|teardrop build]]')
            
    return links

def extract_title_from_content(content):
    """Extract title from markdown content."""
    lines = content.split('\n')
    for line in lines:
        if line.startswith('# '):
            return line[2:].strip()
    return "Untitled"

def has_frontmatter(content):
    """Check if content already has frontmatter."""
    return content.strip().startswith('---')

def add_frontmatter_to_file(file_path):
    """Add frontmatter to a single file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        if has_frontmatter(content):
            print(f"Skipping {file_path} - already has frontmatter")
            return
        
        # Extract title and determine metadata
        title = extract_title_from_content(content)
        tags = determine_tags(file_path, content)
        aliases = generate_aliases(file_path, title)
        see_also = generate_see_also_links(file_path, content)
        
        # Get creation date (use file modification time as approximation)
        try:
            mtime = os.path.getmtime(file_path)
            created_date = datetime.fromtimestamp(mtime).strftime('%Y-%m-%d')
        except:
            created_date = '2026-02-10'
        
        # Build frontmatter
        frontmatter = ['---']
        frontmatter.append(f'created: {created_date}')
        frontmatter.append('tags:')
        for tag in tags:
            frontmatter.append(f'  - {tag}')
        
        if aliases:
            frontmatter.append('aliases:')
            for alias in aliases:
                frontmatter.append(f'  - {alias}')
        
        frontmatter.append('---')
        frontmatter.append('')
        
        # Find the title line and add see_also after it
        lines = content.split('\n')
        new_content = []
        title_added = False
        
        for i, line in enumerate(lines):
            new_content.append(line)
            if line.startswith('# ') and not title_added and see_also:
                new_content.append('')
                new_content.append('> See also: ' + ', '.join(see_also))
                new_content.append('')
                title_added = True
        
        # Combine frontmatter with content
        final_content = '\n'.join(frontmatter) + '\n'.join(new_content)
        
        with open(file_path, 'w', encoding='utf-8') as f:
            f.write(final_content)
        
        print(f"Updated {file_path}")
        
    except Exception as e:
        print(f"Error processing {file_path}: {e}")

def main():
    """Process all markdown files in akron and autarkeia domains."""
    base_path = Path("/mnt/ssd/aletheia/theke")
    
    # Process akron files
    akron_files = list((base_path / "akron").glob("**/*.md"))
    
    # Process autarkeia files  
    autarkeia_files = list((base_path / "autarkeia").glob("**/*.md"))
    
    all_files = akron_files + autarkeia_files
    
    print(f"Found {len(all_files)} markdown files to process")
    
    for file_path in sorted(all_files):
        add_frontmatter_to_file(file_path)

if __name__ == "__main__":
    main()