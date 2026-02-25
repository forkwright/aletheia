# Local Venue Discovery with Multi-Source Verification
Find and verify local businesses matching specific criteria by combining search queries with detailed reviews.

## When to Use
When you need to discover local venues (restaurants, bars, cafes, etc.) in a specific location that meet multiple criteria (amenities, atmosphere, services), and want to validate findings with detailed information before recommending them.

## Steps
1. Conduct broad search query for venue type + location + key criteria (e.g., "wine bar Austin TX laptop work wifi")
2. Conduct targeted searches for promising candidate venues found in results
3. Fetch detailed review pages (e.g., from Wanderlog) for top candidates to verify amenities and get user feedback
4. Compare multiple venues across the criteria to identify best matches
5. Return venues with verified details from review sources

## Tools Used
- exec: Perform web searches via curl with custom User-Agent headers to find venue candidates
- web_fetch: Retrieve detailed review and information pages for specific venues to verify amenities and quality
