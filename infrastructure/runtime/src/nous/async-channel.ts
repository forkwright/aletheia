// Async push/pull channel for bridging callbacks with async generators
export class AsyncChannel<T> {
  private queue: T[] = [];
  private waiting: ((done: boolean) => void) | null = null;
  private closed = false;

  push(item: T): void {
    if (this.closed) return;
    this.queue.push(item);
    this.waiting?.(false);
    this.waiting = null;
  }

  close(): void {
    this.closed = true;
    this.waiting?.(true);
    this.waiting = null;
  }

  async *[Symbol.asyncIterator](): AsyncGenerator<T> {
    while (true) {
      while (this.queue.length > 0) yield this.queue.shift()!;
      if (this.closed) return;
      const done = await new Promise<boolean>((r) => {
        this.waiting = r;
      });
      if (done && this.queue.length === 0) return;
    }
  }
}
