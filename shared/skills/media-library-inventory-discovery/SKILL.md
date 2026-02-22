# Media Library Inventory Discovery
Systematically locate and catalog media storage locations across a networked file system, identifying directory structures and quantifying content distribution.

## When to Use
When you need to understand the organization and scope of a distributed media library, identify all media storage locations on a network, or generate an inventory baseline before performing operations like cleanup, migration, or analysis.

## Steps
1. List the primary media directory to identify top-level categories
2. Scan the parent directory to discover alternative or related media storage paths
3. Examine secondary storage locations (e.g., staging areas, separate shares) to understand the full landscape
4. Count entries in each major subdirectory to quantify content volume and identify which categories have actual data

## Tools Used
- exec: Used to run ls commands for directory exploration and wc for counting entries across the filesystem hierarchy