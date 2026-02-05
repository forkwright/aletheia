#!/usr/bin/env python3
"""
Torque Specification FSM Verification Script
Queries the RAG system to verify torque specifications from the Factory Service Manual
against our documented values.
"""

import os
import sys
import lancedb
from pathlib import Path
from sentence_transformers import SentenceTransformer
from datetime import datetime
import re

# Set up paths
script_dir = Path(__file__).parent
rag_path = Path('/mnt/ssd/moltbot/dianoia/autarkeia/praxis/vehicle/dodge_ram_2500_1997/manual_processing')
db_path = rag_path / 'database' / 'manuals_vector.lancedb'

# Torque specs to verify (our current documented values)
TORQUE_SPECS_TO_VERIFY = [
    {
        'name': 'Pitman arm nut',
        'our_value': '185 ft-lbs',
        'search_terms': ['pitman arm nut torque', 'pitman arm fastener', 'steering pitman nut']
    },
    {
        'name': 'Steering box to frame bolts',
        'our_value': '95 ft-lbs',
        'search_terms': ['steering box frame bolts', 'steering gear frame', 'steering box mounting']
    },
    {
        'name': 'Wheel lug nuts',
        'our_value': '135 ft-lbs',
        'search_terms': ['wheel lug nuts torque', 'lug nut specification', 'wheel fastener torque']
    },
    {
        'name': 'Injector hold-down nuts',
        'our_value': '89 in-lbs',
        'search_terms': ['injector hold down', 'fuel injector clamp', 'injector fastener']
    },
    {
        'name': 'Valve cover bolts',
        'our_value': '18 ft-lbs',
        'search_terms': ['valve cover bolts', 'valve cover torque', 'rocker cover bolts']
    },
    {
        'name': 'Oil pan bolts',
        'our_value': '18 ft-lbs',
        'search_terms': ['oil pan bolts', 'oil pan torque', 'oil sump fasteners']
    }
]

# Additional steering-related searches
STEERING_SEARCHES = [
    'steering shaft clamp torque',
    'drag link torque',
    'tie rod end torque',
    'track bar bolt torque',
    'power steering line torque',
    'steering column torque'
]

def extract_torque_values(text):
    """Extract torque values from text using regex patterns."""
    patterns = [
        r'(\d+(?:\.\d+)?)\s*(?:ft\.?-?lbs?|foot-pounds?)',  # ft-lbs
        r'(\d+(?:\.\d+)?)\s*(?:in\.?-?lbs?|inch-pounds?)',  # in-lbs
        r'(\d+(?:\.\d+)?)\s*(?:N\.?m|newton-meters?)',      # Nm
        r'(\d+(?:\.\d+)?)\s*(?:lb\.?-?ft|pound-feet)',      # lb-ft
    ]
    
    torque_values = []
    for pattern in patterns:
        matches = re.finditer(pattern, text, re.IGNORECASE)
        for match in matches:
            # Get surrounding context (50 chars before and after)
            start = max(0, match.start() - 50)
            end = min(len(text), match.end() + 50)
            context = text[start:end].strip()
            
            torque_values.append({
                'value': match.group(0),
                'numeric': float(match.group(1)),
                'context': context
            })
    
    return torque_values

def search_rag_system(query, limit=5):
    """Search the RAG system for a given query."""
    try:
        # Load embedding model
        model = SentenceTransformer('BAAI/bge-base-en-v1.5')
        
        # Connect to database
        db = lancedb.connect(str(db_path))
        table = db.open_table("manuals")
        
        # Embed query
        query_vector = model.encode(query, normalize_embeddings=True).tolist()
        
        # Search
        results = table.search(query_vector).limit(limit).to_list()
        
        return results
        
    except Exception as e:
        print(f"Error searching RAG system: {e}")
        return []

def main():
    """Main verification process."""
    
    # Check if RAG system is available
    if not db_path.exists():
        print(f"❌ RAG database not found at: {db_path}")
        return
        
    print("🔍 Starting Torque Specification FSM Verification")
    print(f"📅 Date: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    print("="*70)
    
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
        
        spec_results = []
        
        # Try each search term
        for search_term in spec['search_terms']:
            print(f"  Searching: '{search_term}'")
            rag_results = search_rag_system(search_term, limit=3)
            
            for i, result in enumerate(rag_results, 1):
                relevance = (1 - result['_distance']) * 100
                print(f"    Result {i}: {relevance:.1f}% relevance")
                print(f"    Source: {result['breadcrumbs']}")
                
                # Extract torque values from this result
                torque_values = extract_torque_values(result['text'])
                
                if torque_values:
                    print(f"    Found torque values: {[tv['value'] for tv in torque_values]}")
                    for tv in torque_values:
                        spec_results.append({
                            'search_term': search_term,
                            'relevance': relevance,
                            'source': result['breadcrumbs'],
                            'torque_value': tv['value'],
                            'numeric_value': tv['numeric'],
                            'context': tv['context'],
                            'full_text': result['text'][:500]  # First 500 chars
                        })
                else:
                    print(f"    No torque values found in this result")
        
        # Analyze results for this specification
        if spec_results:
            # Filter for most relevant results (>50% relevance)
            relevant_results = [r for r in spec_results if r['relevance'] > 50]
            
            if relevant_results:
                # Group by torque value to find consensus
                value_groups = {}
                for result in relevant_results:
                    val = result['torque_value']
                    if val not in value_groups:
                        value_groups[val] = []
                    value_groups[val].append(result)
                
                # Find most commonly found value
                most_common = max(value_groups.keys(), key=lambda k: len(value_groups[k]))
                
                results['verified_specs'].append({
                    'name': spec['name'],
                    'our_value': spec['our_value'],
                    'fsm_value': most_common,
                    'confidence': len(value_groups[most_common]),
                    'sources': [r['source'] for r in value_groups[most_common]],
                    'all_results': spec_results
                })
                
                # Check for discrepancy
                if most_common.lower() not in spec['our_value'].lower():
                    results['discrepancies'].append({
                        'name': spec['name'],
                        'our_value': spec['our_value'],
                        'fsm_value': most_common,
                        'sources': value_groups[most_common]
                    })
                    print(f"    ⚠️  DISCREPANCY: Our {spec['our_value']} vs FSM {most_common}")
                else:
                    print(f"    ✅ VERIFIED: {spec['our_value']} matches FSM")
            else:
                results['not_found'].append({
                    'name': spec['name'],
                    'our_value': spec['our_value'],
                    'reason': 'No relevant results (all <50% relevance)'
                })
                print(f"    ❓ NOT FOUND: No relevant FSM data")
        else:
            results['not_found'].append({
                'name': spec['name'],
                'our_value': spec['our_value'],
                'reason': 'No torque values found in search results'
            })
            print(f"    ❌ NOT FOUND: No torque values in search results")
    
    # Search for additional steering-related torques
    print(f"\n🔍 Searching for additional steering-related torques...")
    print("-" * 50)
    
    for search_term in STEERING_SEARCHES:
        print(f"  Searching: '{search_term}'")
        rag_results = search_rag_system(search_term, limit=2)
        
        for result in rag_results:
            relevance = (1 - result['_distance']) * 100
            if relevance > 60:  # Only high-relevance results
                torque_values = extract_torque_values(result['text'])
                if torque_values:
                    results['additional_steering'].append({
                        'search_term': search_term,
                        'relevance': relevance,
                        'source': result['breadcrumbs'],
                        'torque_values': torque_values,
                        'text_snippet': result['text'][:300]
                    })
    
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
        f.write(f"**Source:** Factory Service Manual RAG System\n\n")
        
        f.write("## Summary\n\n")
        f.write(f"- **Verified specifications:** {len(results['verified_specs'])}\n")
        f.write(f"- **Discrepancies found:** {len(results['discrepancies'])}\n")
        f.write(f"- **Not found in FSM:** {len(results['not_found'])}\n")
        f.write(f"- **Additional steering torques:** {len(results['additional_steering'])}\n\n")
        
        # Discrepancies section
        if results['discrepancies']:
            f.write("## ⚠️ DISCREPANCIES FOUND\n\n")
            for disc in results['discrepancies']:
                f.write(f"### {disc['name']}\n")
                f.write(f"- **Our documented value:** {disc['our_value']}\n")
                f.write(f"- **FSM value:** {disc['fsm_value']}\n")
                f.write(f"- **Sources:** {', '.join([s['source'] for s in disc['sources']])}\n")
                f.write(f"- **Action needed:** Update documentation to match FSM\n\n")
        else:
            f.write("## ✅ No Discrepancies Found\n\n")
            f.write("All verified specifications match our documentation.\n\n")
        
        # Verified specifications
        f.write("## Verified Specifications\n\n")
        for spec in results['verified_specs']:
            f.write(f"### {spec['name']}\n")
            f.write(f"- **Our value:** {spec['our_value']}\n")
            f.write(f"- **FSM value:** {spec['fsm_value']}\n")
            f.write(f"- **Confidence:** {spec['confidence']} matching results\n")
            f.write(f"- **Sources:** {', '.join(set(spec['sources']))}\n\n")
        
        # Not found specifications
        if results['not_found']:
            f.write("## ❓ Specifications Not Found in FSM\n\n")
            for spec in results['not_found']:
                f.write(f"### {spec['name']}\n")
                f.write(f"- **Our value:** {spec['our_value']}\n")
                f.write(f"- **Reason:** {spec['reason']}\n")
                f.write(f"- **Action:** Manual verification needed\n\n")
        
        # Additional steering torques
        if results['additional_steering']:
            f.write("## Additional Steering-Related Torques\n\n")
            for item in results['additional_steering']:
                f.write(f"### Search: {item['search_term']}\n")
                f.write(f"- **Relevance:** {item['relevance']:.1f}%\n")
                f.write(f"- **Source:** {item['source']}\n")
                f.write(f"- **Torque values found:**\n")
                for tv in item['torque_values']:
                    f.write(f"  - {tv['value']} (Context: ...{tv['context']}...)\n")
                f.write("\n")
        
        f.write("## Recommendations\n\n")
        
        if results['discrepancies']:
            f.write("1. **IMMEDIATE ACTION:** Update torque specification documentation for discrepancies found\n")
            f.write("2. **Verify:** Double-check FSM sources for accuracy\n")
            f.write("3. **Update:** Modify any work procedures that use incorrect values\n\n")
        
        if results['not_found']:
            f.write("4. **Manual verification:** Research specifications not found by RAG system\n")
            f.write("5. **Cross-reference:** Check additional service manual sections\n\n")
        
        f.write("6. **Documentation:** Consider adding any new torque specifications found\n")
        f.write("7. **Review:** Periodic re-verification of critical specifications\n\n")
        
        f.write("---\n")
        f.write("*Generated by Akron torque specification verification system*\n")

if __name__ == "__main__":
    main()