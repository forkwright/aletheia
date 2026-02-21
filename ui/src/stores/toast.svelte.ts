// Toast notification queue — auto-dismiss after 5s
interface ToastItem {
  id: string;
  agentName: string;
  emoji?: string | null;
  preview: string;
  agentId?: string;
}

let toasts = $state<ToastItem[]>([]);

export function showToast(
  agentName: string,
  emoji: string | null | undefined,
  preview: string,
  agentId?: string,
): void {
  const id = `toast-${Date.now()}`;
  toasts = [...toasts, { id, agentName, emoji, preview, agentId }];
  setTimeout(() => {
    toasts = toasts.filter((t) => t.id !== id);
  }, 5000);
}

export function getToasts(): ToastItem[] {
  return toasts;
}

export function dismissToast(id: string): void {
  toasts = toasts.filter((t) => t.id !== id);
}
