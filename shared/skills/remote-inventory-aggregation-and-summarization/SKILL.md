# Remote Inventory Aggregation and Summarization
Remotely access distributed inventory files on a server, aggregate their contents, and generate a consolidated summary document.

## When to Use
When you need to collect and summarize inventory data spread across multiple files on a remote system, such as clothing, equipment, supplies, or other categorized items stored in separate markdown or text files.

## Steps
1. Connect to remote server via SSH and list the target directory to identify all inventory files
2. Retrieve the contents of all relevant inventory files from the remote location using cat or similar commands
3. Parse and extract key information from the retrieved files (items, quantities, descriptions)
4. Create a consolidated summary document that aggregates the data by category with item counts
5. Store the summary output locally for further use or analysis

## Tools Used
- exec: used to run SSH commands for remote file system access and to create local summary files