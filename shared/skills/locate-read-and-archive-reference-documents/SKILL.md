# Locate, Read, and Archive Reference Documents
Find a document in a standard location, review its contents, and archive it to a project-specific reference directory.

## When to Use
When you need to retrieve a reference document from a default storage location (like Downloads), verify its contents, and then store it in a dedicated project or domain-specific reference folder for future access.

## Steps
1. Search for the document in the source location using a pattern match (e.g., ~/Downloads/)
2. Read and verify the document contents to confirm it's the correct resource
3. Create the destination directory structure if it doesn't exist
4. Copy the document to the project reference location

## Tools Used
- exec: to search for files and create directories with mkdir -p
- read: to verify document contents before archiving
- exec: to copy the file to its final destination