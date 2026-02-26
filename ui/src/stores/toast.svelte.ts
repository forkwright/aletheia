// Toast notification queue — auto-dismiss after 5s
interface ToastItem {
  id: string;
  agentName: string;
  emoji?: string | null;
  preview: string;
  agentId?: string;
}

let toasts = $state<ToastItem[]>([]);
const timeouts = new Map<string, ReturnType<typeof setTimeout>>();

export function showToast(
  agentName: string,
  emoji: string | null | undefined,
  preview: string,
  agentId?: string,
): void {
  const id = `toast-${Date.now()}`;
  toasts = [...toasts, { id, agentName, ...(emoji !== undefined && { emoji }), preview, ...(agentId !== undefined && { agentId }) }];
  timeouts.set(id, setTimeout(() => {
    toasts = toasts.filter((t) => t.id !== id);
    timeouts.delete(id);
  }, 5000));
}

export function getToasts(): ToastItem[] {
  return toasts;
}

export function dismissToast(id: string): void {
  const tid = timeouts.get(id);
  if (tid !== undefined) {
    clearTimeout(tid);
    timeouts.delete(id);
  }
  toasts = toasts.filter((t) => t.id !== id);
}
