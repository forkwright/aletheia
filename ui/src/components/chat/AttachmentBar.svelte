<script lang="ts">
  import type { MediaItem } from "../../lib/types";
  import { isImageType, isTextLikeType } from "../../lib/media";

  let { attachments, onRemove }: {
    attachments: MediaItem[];
    onRemove: (index: number) => void;
  } = $props();
</script>

{#if attachments.length > 0}
  <div class="attachment-preview">
    {#each attachments as att, i (i)}
      <div class="attachment-thumb">
        {#if isImageType(att.contentType)}
          <img src="data:{att.contentType};base64,{att.data}" alt={att.filename ?? "attachment"} />
        {:else}
          <div class="file-icon">
            {#if att.contentType === "application/pdf"}
              <span class="file-emoji">📄</span>
            {:else if isTextLikeType(att.contentType)}
              <span class="file-emoji">📝</span>
            {:else}
              <span class="file-emoji">📎</span>
            {/if}
          </div>
        {/if}
        <button class="remove-btn" onclick={() => onRemove(i)} aria-label="Remove attachment">×</button>
        {#if att.filename}
          <span class="attachment-name">{att.filename}</span>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .attachment-preview {
    display: flex;
    gap: 8px;
    padding: 8px 0;
    flex-wrap: wrap;
  }
  .attachment-thumb {
    position: relative;
    width: 80px;
    height: 80px;
    border-radius: var(--radius-sm);
    overflow: hidden;
    border: 1px solid var(--border);
    background: var(--surface);
  }
  .attachment-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .file-icon {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--surface);
  }
  .file-emoji {
    font-size: var(--text-3xl);
  }
  .attachment-thumb .remove-btn {
    position: absolute;
    top: 2px;
    right: 2px;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--overlay-dark);
    border: none;
    color: #fff;
    font-size: var(--text-base);
    line-height: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    opacity: 0;
    transition: opacity var(--transition-quick);
  }
  .attachment-thumb:hover .remove-btn {
    opacity: 1;
  }
  .attachment-thumb .attachment-name {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    padding: 2px 4px;
    background: var(--overlay-dark);
    color: #fff;
    font-size: var(--text-2xs);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @media (max-width: 768px) {
    .attachment-thumb {
      width: 64px;
      height: 64px;
    }
  }
</style>
