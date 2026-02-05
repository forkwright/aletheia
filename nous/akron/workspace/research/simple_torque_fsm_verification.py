#!/usr/bin/env python3
"""
Simple Torque Specification FSM Verification Script
Directly parses the JSON data to find torque specifications without using embeddings.
"""

import json
import re
from datetime import datetime
from pathlib import Path

# Path to the processed JSON data
JSON_PATH = Path('/mnt/ssd/moltbot/dianoia/autarkeia/praxis/vehicle/dodge_ram_2500_1997/manual_processing/processed/combined_docs.json')

# Torque specs to verify (our current documented values)
TORQUE_SPECS_TO_VERIFY = [
    {
        'name': 'Pitman arm nut',
        'our_value': '185 ft-lbs',
        'keywords': ['pitman', 'arm', 'nut', 'steering'],
        'exclude_keywords': []
    },
    {
        'name': 'Steering box to frame bolts',
        'our_value': '95 ft-lbs',
        'keywords': ['steering', 'box', 'frame', 'bolt', 'gear'],
        'exclude_keywords': ['fluid', 'pump', 'line']
    },
    {
        'name': 'Wheel lug nuts',
        'our_value': '135 ft-lbs',
        'keywords': ['wheel', 'lug', 'nut'],
        'exclude_keywords': []
    },
    {
        'name': 'Injector hold-down nuts',
        'our_value': '89 in-lbs',
        'keywords': ['injector', 'hold', 'down', 'clamp'],
        'exclude_keywords': []
    },
    {
        'name': 'Valve cover bolts',
        'our_value': '18 ft-lbs',
        'keywords': ['valve', 'cover', 'bolt', 'rocker'],
        'exclude_keywords': []
    },
    {
        'name': 'Oil pan bolts',
        'our_value': '18 ft-lbs',
        'keywords': ['oil', 'pan', 'bolt'],
        'exclude_keywords': []
    }
]

# Additional steering-related keywords
STEERING_KEYWORDS = ['steering', 'pitman', 'drag link', 'tie rod', 'track bar', 'shaft']

def extract_torque_values(text):
    """Extract torque values from text using regex patterns."""
    # More comprehensive patterns for torque specifications
    patterns = [
        r'(\d+(?:\.\d+)?)\s*(?:ft\.?\-?lbs?|foot\-?pounds?|ft\-lbs)',  # ft-lbs variations
        r'(\d+(?:\.\d+)?)\s*(?:in\.?\-?lbs?|inch\-?pounds?|in\-lbs)',  # in-lbs variations
        r'(\d+(?:\.\d+)?)\s*(?:N\.?m|newton\-?meters?)',               # Nm
        r'(\d+(?:\.\d+)?)\s*(?:lb\.?\-?ft|pound\-?feet)',             # lb-ft
        r'(\d+(?:\.\d+)?)\s*(?:kg\.?\-?m|kilogram\-?meters?)',        # kg-m
        r'(\d+(?:\.\d+)?)\s*(?:torque|spec|specification):\s*(\d+(?:\.\d+)?)\s*(?:ft\.?\-?lbs?|in\.?\-?lbs?)',  # "torque: 185 ft-lbs"
    ]
    
    torque_values = []
    text_lower = text.lower()
    
    for pattern in patterns:
        matches = re.finditer(pattern, text_lower, re.IGNORECASE)
        for match in matches:
            value_str = match.group(0)
            try:
                # Extract numeric value (first number in the match)
                numeric_match = re.search(r'(\d+(?:\.\d+)?)', value_str)
                if numeric_match:
                    numeric = float(numeric_match.group(1))
                    
                    # Get surrounding context (100 chars before and after)
                    start = max(0, match.start() - 100)
                    end = min(len(text), match.end() + 100)
                    context = text[start:end].strip()
                    
                    torque_values.append({
                        'value': value_str,
                        'numeric': numeric,
                        'context': context,
                        'position': match.start()
                    })
            except ValueError:
                continue
    
    return torque_values

def text_contains_keywords(text, keywords, exclude_keywords=None):
    """Check if text contains the specified keywords."""
    text_lower = text.lower()
    
    # Check if exclude keywords are present
    if exclude_keywords:
        for exclude in exclude_keywords:
            if exclude.lower() in text_lower:
                return False
    
    # Check if all keywords are present (flexible matching)
    keyword_matches = 0
    for keyword in keywords:
        if keyword.lower() in text_lower:
            keyword_matches += 1
    
    # Return True if at least 60% of keywords match
    return keyword_matches >= len(keywords) * 0.6

def search_json_for_spec(data, spec_info):
    """Search through JSON data for a specific torque specification."""
    results = []
    
    for entry in data:
        content = entry.get('content', '')
        title = entry.get('title', '')
        breadcrumbs = entry.get('breadcrumbs', '')
        
        # Combine all searchable text
        full_text = f"{title} {breadcrumbs} {content}"
        
        # Check if this entry is relevant to our specification
        if text_contains_keywords(full_text, spec_info['keywords'], spec_info['exclude_keywords']):
            
            # Look for torque values in this entry
            torque_values = extract_torque_values(content)
            
            if torque_values:
                results.append({
                    'title': title,
                    'breadcrumbs': breadcrumbs,
                    'torque_values': torque_values,
                    'content_snippet': content[:500],
                    'relevance_score': len([k for k in spec_info['keywords'] if k.lower() in full_text.lower()])
                })
    
    return results

def search_for_additional_steering(data):
    """Search for any additional steering-related torque specifications."""
    results = []
    
    for entry in data:
        content = entry.get('content', '')
        title = entry.get('title', '')
        breadcrumbs = entry.get('breadcrumbs', '')
        
        full_text = f"{title} {breadcrumbs} {content}".lower()
        
        # Check if this entry is related to steering
        steering_related = any(keyword.lower() in full_text for keyword in STEERING_KEYWORDS)
        
        if steering_related:
            torque_values = extract_torque_values(content)
            
            if torque_values:
                results.append({
                    'title': title,
                    'breadcrumbs': breadcrumbs,
                    'torque_values': torque_values,
                    'content_snippet': content[:300],
                    'steering_keywords_found': [k for k in STEERING_KEYWORDS if k.lower() in full_text]
                })
    
    return results

def main():
    """Main verification process."""
    
    print("🔍 Starting Simple Torque Specification FSM Verification")
    print(f"📅 Date: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    print("="*70)
    
    # Load JSON data
    print("📂 Loading FSM data from JSON...")
    try:
        with open(JSON_PATH, 'r', encoding='utf-8') as f:
            data = json.load(f)
        print(f"✅ Loaded {len(data)} FSM entries")
    except Exception as e:
        print(f"❌ Error loading JSON data: {e}")
        return
    
    results = {
        'verified_specs': [],
        'discrepancies': [],
        'not_found': [],
        'additional_steering': []
    }
    
    # Verify each torque specification
    for spec in TORQUE_SPECS_TO_VERIFY:
        print(f"\n🔧 Checking: {spec['name']} (Our value: {spec['our_value']})")
        print("-" * 50)
        
        search_results = search_json_for_spec(data, spec)
        
        if search_results:
            print(f"  Found {len(search_results)} relevant entries")
            
            # Analyze the torque values found
            all_torque_values = []
            for result in search_results:
                print(f"    📄 {result['breadcrumbs']}")
                for tv in result['torque_values']:
                    print(f"      🔧 {tv['value']} (Context: ...{tv['context'][:50]}...)")
                    all_torque_values.append(tv['value'])
                    
            # Find the most common torque value
            if all_torque_values:
                value_counts = {}
                for val in all_torque_values:
                    val_normalized = val.lower().strip()
                    value_counts[val_normalized] = value_counts.get(val_normalized, 0) + 1
                
                most_common_value = max(value_counts.keys(), key=lambda k: value_counts[k])
                
                results['verified_specs'].append({
                    'name': spec['name'],
                    'our_value': spec['our_value'],
                    'fsm_value': most_common_value,
                    'frequency': value_counts[most_common_value],
                    'all_values_found': list(value_counts.keys()),
                    'sources': [r['breadcrumbs'] for r in search_results]
                })
                
                # Check for discrepancy
                our_value_normalized = spec['our_value'].lower().replace(' ', '').replace('-', '')
                fsm_value_normalized = most_common_value.lower().replace(' ', '').replace('-', '')
                
                if our_value_normalized not in fsm_value_normalized and fsm_value_normalized not in our_value_normalized:
                    results['discrepancies'].append({
                        'name': spec['name'],
                        'our_value': spec['our_value'],
                        'fsm_value': most_common_value,
                        'sources': [r['breadcrumbs'] for r in search_results[:3]]  # Top 3 sources
                    })
                    print(f"    ⚠️  DISCREPANCY: Our {spec['our_value']} vs FSM {most_common_value}")
                else:
                    print(f"    ✅ VERIFIED: {spec['our_value']} matches FSM")
            else:
                results['not_found'].append({
                    'name': spec['name'],
                    'our_value': spec['our_value'],
                    'reason': 'No torque values found in relevant entries'
                })
                print(f"    ❓ NOT FOUND: No torque values in relevant entries")
        else:
            results['not_found'].append({
                'name': spec['name'],
                'our_value': spec['our_value'],
                'reason': 'No relevant FSM entries found'
            })
            print(f"    ❌ NOT FOUND: No relevant entries in FSM")
    
    # Search for additional steering-related torque specifications
    print(f"\n🔍 Searching for additional steering-related torques...")
    print("-" * 50)
    
    additional_results = search_for_additional_steering(data)
    
    # Filter out duplicates and results we've already covered
    covered_specs = [spec['name'].lower() for spec in TORQUE_SPECS_TO_VERIFY]
    
    for result in additional_results:
        # Check if this might be something new
        title_lower = result['title'].lower()
        breadcrumbs_lower = result['breadcrumbs'].lower()
        
        is_new = True
        for covered in covered_specs:
            if any(word in title_lower or word in breadcrumbs_lower for word in covered.split()):
                is_new = False
                break
        
        if is_new and result['torque_values']:
            results['additional_steering'].append(result)
            print(f"    📄 {result['breadcrumbs']}")
            for tv in result['torque_values']:
                print(f"      🔧 {tv['value']}")
    
    # Generate report
    generate_report(results)
    
    print(f"\n✅ Verification complete! Report saved to:")
    print(f"   /mnt/ssd/moltbot/akron/workspace/research/torque-spec-fsm-verification.md")

def generate_report(results):
    """Generate markdown report of findings."""
    
    report_path = Path('/mnt/ssd/moltbot/akron/workspace/research/torque-spec-fsm-verification.md')
    
    with open(report_path, 'w') as f:
        f.write("# Torque Specification FSM Verification Report\n\n")
        f.write(f"**Date:** {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n")
        f.write(f"**Vehicle:** 1997 Dodge Ram 2500 5.9L Cummins (VIN: 3B7KF23D9VM592245)\n")
        f.write(f"**Source:** Factory Service Manual JSON Data (Direct Text Search)\n")
        f.write(f"**Method:** Keyword-based search with torque value extraction\n\n")
        
        f.write("## Summary\n\n")
        f.write(f"- **Verified specifications:** {len(results['verified_specs'])}\n")
        f.write(f"- **Discrepancies found:** {len(results['discrepancies'])}\n")
        f.write(f"- **Not found in FSM:** {len(results['not_found'])}\n")
        f.write(f"- **Additional steering torques:** {len(results['additional_steering'])}\n\n")
        
        # Discrepancies section
        if results['discrepancies']:
            f.write("## ⚠️ CRITICAL: DISCREPANCIES FOUND\n\n")
            f.write("**IMMEDIATE ACTION REQUIRED** - The following specifications do not match the FSM:\n\n")
            for disc in results['discrepancies']:
                f.write(f"### {disc['name']}\n")
                f.write(f"- **❌ Our documented value:** {disc['our_value']}\n")
                f.write(f"- **✅ FSM value:** {disc['fsm_value']}\n")
                f.write(f"- **Sources:** {', '.join(disc['sources'][:3])}\n")
                f.write(f"- **⚠️ Action needed:** **UPDATE DOCUMENTATION IMMEDIATELY**\n\n")
        else:
            f.write("## ✅ No Critical Discrepancies Found\n\n")
            f.write("All verified specifications match our documentation.\n\n")
        
        # Verified specifications
        if results['verified_specs']:
            f.write("## ✅ Verified Specifications\n\n")
            for spec in results['verified_specs']:
                f.write(f"### {spec['name']}\n")
                f.write(f"- **Our value:** {spec['our_value']}\n")
                f.write(f"- **FSM value:** {spec['fsm_value']}\n")
                f.write(f"- **Confidence:** Found {spec['frequency']} time(s)\n")
                if len(spec['all_values_found']) > 1:
                    f.write(f"- **Other values found:** {', '.join([v for v in spec['all_values_found'] if v != spec['fsm_value']])}\n")
                f.write(f"- **Sources:** {', '.join(set(spec['sources'][:3]))}\n\n")
        
        # Not found specifications
        if results['not_found']:
            f.write("## ❓ Specifications Not Found in FSM\n\n")
            for spec in results['not_found']:
                f.write(f"### {spec['name']}\n")
                f.write(f"- **Our value:** {spec['our_value']}\n")
                f.write(f"- **Reason:** {spec['reason']}\n")
                f.write(f"- **Action:** Manual verification needed - check specific FSM sections\n\n")
        
        # Additional steering torques
        if results['additional_steering']:
            f.write("## Additional Steering-Related Torques Found\n\n")
            f.write("These torque specifications were found in steering-related sections but are not in our documentation:\n\n")
            
            for item in results['additional_steering']:
                f.write(f"### {item['title']}\n")
                f.write(f"- **Source:** {item['breadcrumbs']}\n")
                f.write(f"- **Torque values found:**\n")
                for tv in item['torque_values']:
                    f.write(f"  - **{tv['value']}** (Context: ...{tv['context'][:60]}...)\n")
                f.write(f"- **Keywords found:** {', '.join(item['steering_keywords_found'])}\n\n")
        
        f.write("## Recommendations\n\n")
        
        if results['discrepancies']:
            f.write("### 🚨 IMMEDIATE ACTIONS REQUIRED:\n\n")
            f.write("1. **CRITICAL:** Update torque specification documentation for ALL discrepancies found\n")
            f.write("2. **VERIFY:** Double-check FSM sources and confirm accuracy\n")
            f.write("3. **AUDIT:** Review any recent work that used the incorrect specifications\n")
            f.write("4. **COMMUNICATE:** Notify anyone working on these systems of the corrections\n\n")
        
        if results['not_found']:
            f.write("### 📋 Follow-up Actions:\n\n")
            f.write("5. **Manual verification:** Research specifications not found by automated search\n")
            f.write("6. **Cross-reference:** Check specific FSM sections mentioned in torque-specifications.md\n")
            f.write("7. **Validate sources:** Confirm current documentation sources are accurate\n\n")
        
        if results['additional_steering']:
            f.write("### 📈 Documentation Enhancement:\n\n")
            f.write("8. **Consider adding:** New torque specifications found in FSM search\n")
            f.write("9. **Review completeness:** Assess if steering section documentation is comprehensive\n\n")
        
        f.write("### 🔄 Process Improvements:\n\n")
        f.write("10. **Periodic verification:** Schedule regular FSM cross-checks\n")
        f.write("11. **Source validation:** Always verify torque specs against FSM before work\n")
        f.write("12. **Documentation discipline:** Update docs immediately when discrepancies are found\n\n")
        
        f.write("---\n")
        f.write("*Generated by Akron torque specification verification system*\n")
        f.write("*Method: Direct JSON parsing with keyword matching and torque value extraction*\n")

if __name__ == "__main__":
    main()