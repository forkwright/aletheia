// Cross-agent notification state — unread badges + notification history
interface Notification {
  id: string;
  agentId: string;
  agentName: string;
  preview: string;
  timestamp: number;
  read: boolean;
}

let notifications = $state<Notification[]>([]);

export function addNotification(agentId: string, agentName: string, preview: string): void {
  const id = `notif-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
  notifications = [
    { id, agentId, agentName, preview, timestamp: Date.now(), read: false },
    ...notifications,
  ].slice(0, 50);
}

export function getUnreadCount(agentId: string): number {
  return notifications.filter((n) => n.agentId === agentId && !n.read).length;
}

export function getTotalUnreadCount(): number {
  return notifications.filter((n) => !n.read).length;
}

export function markRead(agentId: string): void {
  notifications = notifications.map((n) =>
    n.agentId === agentId && !n.read ? { ...n, read: true } : n,
  );
}

export function getNotifications(): Notification[] {
  return notifications;
}
