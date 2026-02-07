#!/usr/bin/env python3
"""
Script to systematically add frontmatter, tags, and links to ardent/ markdown files
Following the pattern established in epimeleia/
"""

import os
import re
import subprocess
from datetime import datetime
from pathlib import Path

def get_file_creation_date(file_path):
    """Get file creation date or use a reasonable default"""
    try:
        # Try git log first (most accurate for content creation)
        result = subprocess.run(['git', 'log', '--follow', '--format=%ai', '--', str(file_path)], 
                              capture_output=True, text=True, cwd='/mnt/ssd/aletheia/theke')
        if result.returncode == 0 and result.stdout.strip():
            # Get the oldest commit date
            dates = result.stdout.strip().split('\n')
            if dates and dates[-1]:
                return dates[-1][:10]  # YYYY-MM-DD
    except:
        pass
    
    # Fallback to file modification date
    try:
        stat = os.stat(file_path)
        return datetime.fromtimestamp(stat.st_mtime).strftime('%Y-%m-%d')
    except:
        return '2025-01-01'  # Safe default

def determine_type_tag(content, file_path):
    """Determine the type tag based on content and filename"""
    content_lower = content.lower()
    filename = os.path.basename(file_path).lower()
    
    if 'readme' in filename:
        return 'index'
    elif any(word in filename for word in ['inventory', 'specs', 'sources', 'reference']):
        return 'reference'
    elif any(word in content_lower[:1000] for word in ['step by step', 'process', 'procedure', 'how to']):
        return 'procedure'
    elif any(word in content_lower[:1000] for word in ['analysis', 'market', 'research', 'findings']):
        return 'analysis'
    elif any(word in filename for word in ['plan', 'strategy', 'roadmap']):
        return 'project'
    elif any(word in content_lower[:500] for word in ['philosophy', 'credo', 'belief']):
        return 'personal'
    elif re.search(r'\d{4}-\d{2}-\d{2}', filename) or 'log' in filename:
        return 'log'
    else:
        return 'reference'  # Default

def determine_topic_tags(content, file_path):
    """Determine topic tags based on content and path"""
    tags = ['craft']  # Default for ardent domain
    content_lower = content.lower()
    path_lower = str(file_path).lower()
    
    # Additional topic tags based on content/path
    if any(word in path_lower for word in ['finance', 'cost', 'tax', 'business', 'market']):
        tags.append('career')
    if any(word in path_lower for word in ['dye', 'recipe']):
        tags.append('writing')  # Same naming philosophy
    if 'therapy' in content_lower or 'audhd' in content_lower:
        tags.append('audhd')
    
    return tags

def generate_aliases(file_path, content):
    """Generate reasonable aliases for linking"""
    filename = os.path.basename(file_path)
    name_without_ext = os.path.splitext(filename)[0]
    
    aliases = []
    
    # Clean up the filename for an alias
    clean_name = name_without_ext.replace('-', ' ').replace('_', ' ')
    if clean_name != name_without_ext:
        aliases.append(clean_name)
    
    # Extract title from content if available
    lines = content.split('\n')[:10]  # Look in first 10 lines
    for line in lines:
        if line.startswith('# '):
            title = line[2:].strip()
            if title and title != clean_name and len(title) < 50:
                aliases.append(title)
            break
    
    # Add a short version if the name is long
    if len(name_without_ext) > 15:
        words = name_without_ext.replace('-', ' ').replace('_', ' ').split()
        if len(words) > 2:
            short = ' '.join(words[:2])
            if short not in aliases:
                aliases.append(short)
    
    return aliases[:3]  # Limit to 3 aliases

def find_related_links(file_path, content):
    """Find related files to link to"""
    links = []
    ardent_path = Path('/mnt/ssd/aletheia/theke/ardent')
    current_dir = Path(file_path).parent
    
    # Key cross-domain links
    cross_domain = {
        'belt': ['[[epimeleia/20251031_personal_philosophy|personal philosophy]]'],
        'dye': ['[[ekphrasis/README|writing domain]]'],
        'craft': ['[[epimeleia/audhd-profile|AuDHD profile]]'],
        'materials': ['[[LEATHER_SOURCES|leather sources]]', '[[HARDWARE_SOURCES|hardware sources]]'],
        'construction': ['[[belt-specifications-comprehensive|belt specs]]', '[[MATERIALS_SUMMARY|materials]]'],
        'market': ['[[pricing-strategy|pricing]]', '[[customer-acquisition|customers]]'],
        'business': ['[[LAUNCH-STRATEGY|launch strategy]]', '[[financial_summary_2025|finances]]']
    }
    
    content_lower = content.lower()
    filename_lower = os.path.basename(file_path).lower()
    
    # Add cross-domain links based on content
    for key, domain_links in cross_domain.items():
        if key in content_lower or key in filename_lower:
            links.extend(domain_links[:2])  # Limit to avoid clutter
    
    # Find related files in same directory
    related_files = []
    try:
        for item in current_dir.iterdir():
            if (item.is_file() and item.suffix == '.md' and 
                item.name != os.path.basename(file_path) and 
                not item.name.startswith('README')):
                related_files.append(item)
    except:
        pass
    
    # Add up to 3 related files from same directory
    for related in related_files[:3]:
        rel_name = related.stem
        display_name = rel_name.replace('-', ' ').replace('_', ' ')
        links.append(f'[[{rel_name}|{display_name}]]')
    
    return links[:6]  # Limit total links

def has_frontmatter(content):
    """Check if file already has frontmatter"""
    return content.strip().startswith('---\n')

def process_file(file_path):
    """Process a single markdown file"""
    print(f"Processing: {file_path}")
    
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        if has_frontmatter(content):
            print(f"  Skipping - already has frontmatter")
            return
        
        # Determine metadata
        created_date = get_file_creation_date(file_path)
        type_tag = determine_type_tag(content, file_path)
        topic_tags = determine_topic_tags(content, file_path)
        aliases = generate_aliases(file_path, content)
        related_links = find_related_links(file_path, content)
        
        # Build frontmatter
        frontmatter = "---\n"
        frontmatter += f"created: {created_date}\n"
        frontmatter += "tags:\n"
        frontmatter += f"  - {type_tag}\n"
        for tag in topic_tags:
            frontmatter += f"  - {tag}\n"
        
        if aliases:
            frontmatter += "aliases:\n"
            for alias in aliases:
                frontmatter += f"  - {alias}\n"
        
        frontmatter += "---\n\n"
        
        # Find the first heading to insert "See also" after
        lines = content.split('\n')
        insert_index = 0
        
        # Find first # heading
        for i, line in enumerate(lines):
            if line.strip().startswith('# '):
                insert_index = i + 1
                break
        
        # Insert "See also" section if we have links
        see_also = ""
        if related_links:
            see_also = f"\n> See also: {', '.join(related_links)}\n"
        
        # Construct new content
        if insert_index > 0:
            new_content = (frontmatter + 
                          '\n'.join(lines[:insert_index]) + 
                          see_also + 
                          '\n'.join(lines[insert_index:]))
        else:
            new_content = frontmatter + see_also + content
        
        # Write the updated file
        with open(file_path, 'w', encoding='utf-8') as f:
            f.write(new_content)
        
        print(f"  Added: {type_tag}, {topic_tags}, {len(aliases)} aliases, {len(related_links)} links")
        
    except Exception as e:
        print(f"  Error processing {file_path}: {e}")

def main():
    """Process all markdown files in ardent/"""
    ardent_path = Path('/mnt/ssd/aletheia/theke/ardent')
    
    # Find all .md files
    md_files = []
    for root, dirs, files in os.walk(ardent_path):
        for file in files:
            if file.endswith('.md'):
                md_files.append(os.path.join(root, file))
    
    print(f"Found {len(md_files)} markdown files")
    
    # Process files, skipping those that already have frontmatter
    processed = 0
    skipped = 0
    
    for file_path in sorted(md_files):
        try:
            with open(file_path, 'r', encoding='utf-8') as f:
                content = f.read()
            
            if has_frontmatter(content):
                skipped += 1
                print(f"Skipping {file_path} - already has frontmatter")
                continue
                
            process_file(file_path)
            processed += 1
            
        except Exception as e:
            print(f"Error with {file_path}: {e}")
    
    print(f"\nCompleted: {processed} processed, {skipped} skipped")

if __name__ == '__main__':
    main()