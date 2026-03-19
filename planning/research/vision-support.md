# Vision (Image Input) Support

Research document for adding multimodal image input to Aletheia conversations.

---

## Question

How should Aletheia support image inputs in conversations, given the current text-only user message path? What changes are needed across hermeneus (LLM types), nous (pipeline), mneme (storage), organon (tools), and theatron (TUI)?

---

## Findings

### 1. Current message format audit

#### Hermeneus (LLM types, `crates/hermeneus/src/types.rs`)

Messages use a `Content` enum with two variants:

```rust
pub enum Content {
    Text(String),
    Blocks(Vec<ContentBlock>),
}
```

`ContentBlock` has variants for: `Text`, `ToolUse`, `ToolResult`, `Thinking`, `ServerToolUse`, `WebSearchToolResult`, `CodeExecutionResult`. All are `#[non_exhaustive]`.

**Image support already exists in tool results.** `ToolResultBlock` has `Image { source: ImageSource }` and `Document { source: DocumentSource }` variants. The `view_file` tool in organon uses these to send base64-encoded images and PDFs to the API.

**The gap: `ContentBlock` has no `Image` variant for user messages.** The Anthropic API accepts image content blocks in user messages directly, but the current `ContentBlock` enum cannot represent this. Images can only enter the conversation through tool results today.

#### Nous (pipeline, `crates/nous/src/pipeline.rs`)

`PipelineMessage` is a simplified representation:

```rust
pub struct PipelineMessage {
    pub role: String,
    pub content: String,  // Text only
    pub token_estimate: i64,
}
```

`PipelineInput` also carries content as a plain `String`. The entire pipeline assumes text-only user input. Image content blocks would need to flow from `PipelineInput` through history loading through to `CompletionRequest` construction.

#### Mneme (storage, `crates/mneme/src/schema.rs`)

The `messages` table stores content as `TEXT NOT NULL`:

```sql
CREATE TABLE IF NOT EXISTS messages (
  ...
  content TEXT NOT NULL,
  ...
);
```

The `Message` struct uses `content: String`. No binary storage, no content type column, no attachment table.

#### Theatron (TUI, `crates/theatron/tui/src/view/image.rs`)

The TUI already has image rendering infrastructure:
- Detects terminal graphics protocol (Kitty, Sixel, TrueColor, TextOnly)
- Renders half-block character previews with per-pixel coloring
- Caches rendered images (keyed by path + display width, max 32 entries)
- Detects image file paths mentioned in message text and renders inline previews

This rendering system works from file paths on disk. It does not render base64-encoded image data from message content blocks.

### 2. Anthropic vision API requirements

#### Message format

User messages accept image content blocks alongside text:

```json
{
  "role": "user",
  "content": [
    {
      "type": "image",
      "source": {
        "type": "base64",
        "media_type": "image/jpeg",
        "data": "<base64-encoded>"
      }
    },
    {
      "type": "text",
      "text": "What's in this image?"
    }
  ]
}
```

URL-referenced images are also supported:

```json
{
  "type": "image",
  "source": {
    "type": "url",
    "url": "https://example.com/image.jpg"
  }
}
```

#### Supported formats

| Format | MIME Type | Notes |
|--------|-----------|-------|
| JPEG | `image/jpeg` | Most common, smallest tokens per pixel |
| PNG | `image/png` | Lossless, larger |
| GIF | `image/gif` | Static frames only |
| WebP | `image/webp` | Good compression |

#### Size limits

- Maximum image size: 20 MB per image (before base64 encoding)
- Maximum image dimensions: the API resizes images larger than 1568px on the longest edge
- Images are scaled to fit within a 1568x1568 bounding box while preserving aspect ratio

#### Token cost

Image tokens are calculated from the scaled dimensions:

```
tokens = (width * height) / 750
```

Examples at common resolutions:

| Resolution | Tokens | Cost at $3/MTok input |
|-----------|--------|----------------------|
| 200x200 | ~54 | $0.0002 |
| 800x600 | ~640 | $0.002 |
| 1568x1568 | ~3,280 | $0.010 |
| Screenshot 1920x1080 (scaled to 1568x882) | ~1,843 | $0.006 |

Images are the most token-expensive input type. A single high-resolution image costs as much as 1,800 tokens of text (roughly 1,350 words).

#### Base64 vs URL trade-offs

| Approach | Pros | Cons |
|----------|------|------|
| Base64 | No external dependency, works offline, deterministic | 33% size overhead, large payloads, stored in messages |
| URL | Smaller request, no encoding overhead | Requires accessible URL, non-deterministic (image can change), network dependency |

**Recommendation: base64 for user-provided files, URL for web references.** Aletheia operates locally; most image inputs will be files on disk. Base64 is the right default. URL support can be added later for web search integration.

### 3. integration points

#### 3.1 message model changes

**Add `Image` variant to `ContentBlock`:**

```rust
pub enum ContentBlock {
    // existing variants...

    /// Image content block (user messages).
    #[serde(rename = "image")]
    Image { source: ImageSource },
}
```

The `ImageSource` struct already exists and handles base64 serialization. Adding the variant to `ContentBlock` is the minimal change needed for the wire format.

**Extend `ImageSource` for URL references (future):**

```rust
pub enum ImageSource {
    Base64 {
        media_type: String,
        data: String,
    },
    Url {
        url: String,
    },
}
```

This is not needed for phase 1 (the current struct already works for base64), but should be planned for the type to avoid a breaking change later.

#### 3.2 pipeline changes

`PipelineMessage` and `PipelineInput` need multimodal content support. Two approaches:

**Option A: Extend PipelineMessage with optional attachments**

```rust
pub struct PipelineMessage {
    pub role: String,
    pub content: String,
    pub token_estimate: i64,
    pub attachments: Vec<Attachment>,
}

pub struct Attachment {
    pub media_type: String,
    pub data: Vec<u8>,  // Raw bytes, encode to base64 at wire boundary
}
```

**Option B: Use hermeneus `Content` directly**

```rust
pub struct PipelineMessage {
    pub role: String,
    pub content: Content,  // hermeneus Content enum
    pub token_estimate: i64,
}
```

**Recommendation: Option A.** Keeping raw bytes in the pipeline and encoding at the wire boundary follows the "format at the boundary" principle from STANDARDS.md. It also avoids coupling the pipeline to the hermeneus wire format.

#### 3.3 token estimation

The token estimator needs to account for image tokens. Currently, `PipelineMessage.token_estimate` is computed from text length. Image tokens follow the formula:

```
image_tokens = (scaled_width * scaled_height) / 750
```

The pipeline must:
1. Compute scaled dimensions (scale to fit 1568x1568 bounding box)
2. Calculate token cost using the formula
3. Include image tokens in the history budget calculation

This prevents the context window from overflowing when multiple images are in the conversation history.

#### 3.4 image preprocessing

Before sending to the API, images should be preprocessed to:
1. **Validate format** (JPEG, PNG, GIF, WebP only)
2. **Check size** (reject files > 20 MB)
3. **Resize if needed** (scale to fit 1568x1568 bounding box before sending, reducing payload size)
4. **Convert unsupported formats** (BMP to PNG)

The `image` crate (`image = "0.25"`) is already a dependency of the theatron TUI crate. It handles loading, resizing, and format conversion. It should be added as a workspace dependency for shared use.

#### 3.5 storage Strategy

Three options for persisting images in mneme:

**Option A: Store base64 inline in message content column**

- Pro: Zero schema change, roundtrip works immediately
- Con: Bloats SQLite, slow queries, duplicates data if image is referenced again

**Option B: Separate attachments table with binary storage**

```sql
CREATE TABLE IF NOT EXISTS message_attachments (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  media_type TEXT NOT NULL,
  data BLOB NOT NULL,
  width INTEGER,
  height INTEGER,
  token_estimate INTEGER DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE(message_id, seq)
);
```

- Pro: Clean separation, BLOB storage is efficient in SQLite, queryable metadata
- Con: Schema migration, join needed for message reconstruction

**Option C: File-based storage with database references**

Store images on disk in `$XDG_DATA_HOME/aletheia/attachments/<hash>.ext` and reference by hash in the database.

- Pro: SQLite stays small, filesystem handles binary well, deduplication by content hash
- Con: Two storage systems to manage, orphan cleanup needed, backup complexity

**Recommendation: Option B for phase 1.** SQLite handles BLOBs well up to moderate sizes. The message content column stores a placeholder (`[image:1]`) while the actual binary lives in the attachments table. This keeps existing text queries fast while supporting binary data. Option C is the right choice if images become large or frequent, but it adds operational complexity that is not justified until storage pressure is measured.

#### 3.6 TUI rendering

The theatron image system renders from file paths detected in text. To render images from message content blocks:

1. **History loading**: When loading history messages, parse content blocks and extract image attachments
2. **Inline rendering**: Use the existing half-block renderer but from in-memory image data rather than file paths
3. **Placeholder text**: For TextOnly terminals, show `[image: 800x600 jpeg, 340 KB]`
4. **Streaming**: Image blocks in user messages are sent before streaming begins, so no streaming-specific handling is needed

The existing `load_and_render_halfblocks` function works with `image::DynamicImage`. Add a parallel path that decodes base64 data to `DynamicImage` instead of loading from disk.

#### 3.7 tool results with images

Already working. The `view_file` tool returns `ToolResultBlock::Image` with base64-encoded data, and the wire serialization handles it. No changes needed for tool-result images.

#### 3.8 user input path

Users need a way to attach images. Options:

1. **File path in message**: `/attach /path/to/image.png` or detecting image paths in user input (theatron already detects image paths in text)
2. **Drag-and-drop**: Terminal paste (some terminals support bracketed paste with binary data, but this is unreliable)
3. **API endpoint**: For pylon (HTTP API), accept multipart form data with image files

**Recommendation: File path command for TUI, multipart upload for API.** A `/attach` or `/image` command in the TUI is the lowest-effort first step. The TUI already detects image paths; extending this to actually attach them to the outgoing message is straightforward.

### 4. Rust image processing libraries

| Crate | Purpose | Status |
|-------|---------|--------|
| `image` (0.25) | Load, resize, convert, save images | Already in workspace (theatron dependency) |
| `imageproc` | Advanced processing (filters, edge detection) | Not needed for vision support |
| `webp` | WebP encoding/decoding | `image` crate handles this |
| `mozjpeg` | High-quality JPEG compression | Optional optimization, not needed initially |

The `image` crate is sufficient for all preprocessing needs: format detection, resize, format conversion. It is pure Rust (no C dependencies) and already compiled as part of the workspace.

### 5. observations

- **Debt**: `PipelineMessage.content` is `String` but should be a richer type even for text-only use. Tool results with images are already converted to text summaries (`[image]`) before storage, losing the image data. (`crates/hermeneus/src/types.rs:190`)
- **Debt**: `Content::text()` method joins text and thinking blocks but ignores images, which will return empty for image-only messages. (`crates/hermeneus/src/types.rs:60-78`)
- **Idea**: The image preprocessing (resize to 1568x1568) could be shared between organon's `view_file` tool and the new user-message image path, avoiding duplicate logic.
- **Idea**: Content-addressable image storage (hash-based dedup) would prevent storing the same screenshot multiple times across sessions.
- **Doc gap**: No documentation on the `ToolResultBlock` variants or how images flow through the system. The types are self-documenting but the flow is not obvious.

---

## Recommendations

### Phased Implementation

#### Phase 1: image content block support (Small)

Add the `Image` variant to `ContentBlock`. This enables images in user messages at the wire format level. Scope:

- Add `ContentBlock::Image { source: ImageSource }` variant
- Update `Content::text()` to return `[image]` for image blocks
- Update wire serialization (already handles `ImageSource` in tool results; verify user-message path)
- Add serialization roundtrip tests

Blast radius: `crates/hermeneus/src/types.rs`, `crates/hermeneus/src/types_tests.rs`

#### Phase 2: pipeline and storage (Medium)

Thread image data through the pipeline and persist it. Scope:

- Add `attachments: Vec<Attachment>` to `PipelineMessage` and `PipelineInput`
- Create `message_attachments` table in mneme (schema migration v5)
- Implement image token estimation in the budget calculator
- Update message reconstruction from storage to include attachments
- Update history loading to reconstruct `Content::Blocks` with images

Blast radius: `crates/nous/src/pipeline.rs`, `crates/mneme/src/schema.rs`, `crates/mneme/src/types.rs`, `crates/mneme/src/store/message.rs`, `crates/nous/src/history.rs`, `crates/nous/src/budget.rs`

#### Phase 3: image preprocessing (Small)

Add validation and preprocessing before API submission. Scope:

- Validate format and size on input
- Resize images exceeding 1568x1568 bounding box (reduces token cost and payload size)
- Convert unsupported formats (BMP) to PNG
- Add `image` as a dependency of nous or create a shared preprocessing module

Blast radius: new module in `crates/nous/src/image.rs` or shared crate

#### Phase 4: TUI integration (Medium)

Enable attaching and viewing images in the TUI. Scope:

- Add `/image <path>` command to attach images to the next message
- Render image content blocks in history view using existing half-block renderer
- Decode base64 image data from message content for rendering
- Show image metadata (dimensions, size, token cost) in the status line

Blast radius: `crates/theatron/tui/src/msg.rs`, `crates/theatron/tui/src/view/chat.rs`, `crates/theatron/tui/src/view/image.rs`, `crates/theatron/tui/src/mapping.rs`

#### Phase 5: API endpoint (Small)

Accept image uploads via the pylon HTTP API. Scope:

- Add multipart form data handling to the `/turn` endpoint
- Extract images from multipart body, validate, attach to turn request
- Return image metadata in turn response

Blast radius: `crates/pylon/src/handler/turn.rs`

### Effort estimate

| Phase | Scope | Dependencies |
|-------|-------|-------------|
| Phase 1 | 1 file, ~20 lines | None |
| Phase 2 | 5-6 files, schema migration | Phase 1 |
| Phase 3 | 1-2 files, ~100 lines | Phase 1 |
| Phase 4 | 4 files | Phase 2 |
| Phase 5 | 1 file | Phase 2 |

Phases 1 and 3 can be done in parallel. Phase 4 and 5 can be done in parallel after phase 2.

---

## Gotchas

1. **Token budget explosion.** A single image costs 54-3,280 tokens. Multiple images in conversation history can exhaust the context window. The budget calculator must account for image tokens, and distillation must handle (or drop) image messages.

2. **Base64 payload size.** A 5 MB image becomes ~6.7 MB base64-encoded. With multiple images in a request, the HTTP payload grows large. Consider streaming uploads or chunked encoding if this becomes a bottleneck.

3. **SQLite BLOB performance.** SQLite handles BLOBs well up to ~1 MB. Above that, performance degrades. The preprocessing step (resize to 1568x1568) keeps most images under 1 MB after compression, but this must be enforced.

4. **Distillation of image messages.** The distillation pipeline summarizes conversation history into text. It must either drop images or generate text descriptions. Generating descriptions requires a vision-capable model, creating a recursive dependency. Phase 1 recommendation: drop images during distillation and note their former presence in the summary.

5. **History reconstruction.** When loading conversation history, image attachments must be reconstructed into `Content::Blocks` for the API request. If images are dropped (distilled), the history must gracefully handle missing attachments without breaking the message format.

6. **Cache invalidation.** The TUI image cache keys on `(PathBuf, width)`. In-memory image data from base64 needs a different cache key (message ID + attachment sequence, or content hash).

---

## References

- Anthropic Messages API: vision content blocks documentation
- `crates/hermeneus/src/types.rs`: current `ContentBlock`, `ImageSource`, `ToolResultBlock` definitions
- `crates/organon/src/builtins/view_file.rs`: existing image encoding for tool results
- `crates/theatron/tui/src/view/image.rs`: TUI image rendering infrastructure
- `crates/mneme/src/schema.rs`: current database schema (v4)
- `crates/nous/src/pipeline.rs`: pipeline message types
- `image` crate: https://crates.io/crates/image (already in workspace)
