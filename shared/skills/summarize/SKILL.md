# Summarize Skill

Summarize long-form content: articles, research papers, podcasts, videos.

## Usage

When asked to summarize content:

1. **For URLs**: Use `web_fetch` to get the content
2. **For files**: Use `read` to get the content
3. **For videos/podcasts**: Check if transcript exists or use whisper

## Output Format

Provide summaries in this structure:

```markdown
## Summary: [Title]

**Source:** [URL or filename]
**Length:** [word count / duration]

### Key Points
- Point 1
- Point 2
- Point 3

### Main Argument/Thesis
[One paragraph summary of the core message]

### Notable Quotes
> "Quote 1"
> "Quote 2"

### My Take
[Brief analysis or relevance to user's interests]
```

## Summary Lengths

- **Brief** (default): 3-5 bullet points + 1 paragraph
- **Standard**: Full template above
- **Detailed**: Include subsections, all key arguments, counterpoints

## Domain-Specific Notes

**Research papers**: Include methodology, findings, limitations
**Podcasts**: Note timestamps for key segments if available
**News**: Include context and implications
**Technical docs**: Focus on practical takeaways
