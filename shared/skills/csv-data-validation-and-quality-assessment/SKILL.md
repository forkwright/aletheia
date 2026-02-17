# CSV Data Validation and Quality Assessment

Systematically validate CSV file structure, row counts, column integrity, and data distribution to ensure data quality before processing.

## When to Use
When you need to verify the integrity and correctness of a CSV dataset, check for expected dimensions, validate column presence/order, or analyze the distribution of key fields before downstream processing.

## Steps
1. Locate and identify the target CSV file using filesystem listing commands
2. Inspect the header row to understand available columns
3. Load the CSV and count total rows and columns
4. Verify row count matches expected values
5. Check column order matches expected schema
6. Analyze value distributions for key columns (especially boolean/categorical fields)
7. Identify and report any anomalies or mismatches
8. Flag specific values of interest (e.g., rows with true/false in key fields)

## Tools Used
- exec: Used to run shell commands for file listing, header inspection, and Python scripts for detailed CSV analysis
- Python csv module: Used to parse CSV structure, read all rows, and analyze column distributions programmatically
