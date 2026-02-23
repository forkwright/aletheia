# Trace Asynchronous Message Flow Through Event Handler to API Call
Identify how user input triggers message sending by following the event handling chain from key press through function calls to API streaming setup.

## When to Use
When you need to understand or debug the complete flow of how user actions (like key presses) propagate through an event-driven system to trigger API calls, especially in async architectures with message passing.

## Steps
1. Search for user input handlers (e.g., key press patterns like "Enter", "Submit") to find the entry point
2. Search for the triggered message/action handlers (e.g., "Msg::Submit") to see what function they call
3. Search for the actual function that processes the message (e.g., "send_message") to locate implementation
4. Read the function implementation to identify the API call pattern being used
5. Extract command snippets around the API call to see parameter setup and async channel creation
6. Trace the API call target (URL, endpoint, streaming setup) to understand the full data flow

## Tools Used
- grep: to search for event handlers, message patterns, and function names across the codebase
- read: to get context about imports and overall file structure
- exec (sed): to extract specific line ranges containing implementation details of key functions
