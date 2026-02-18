# Retrieve and Read Multiple Timestamped Transcripts
Locate and read transcript files from a timestamped directory structure to access conversation content.

## When to Use
When you need to access multiple transcript files organized in timestamped directories and want to review their contents sequentially.

## Steps
1. List files in the timestamped directory using wildcard pattern matching (e.g., `ls -la /path/*pattern*/`)
2. Identify the specific subdirectories containing transcript files
3. Read each transcript file using `cat` command, targeting the transcript.txt file in each subdirectory
4. Review the extracted transcript content

## Tools Used
- exec: Used to run shell commands for directory listing and file reading operations