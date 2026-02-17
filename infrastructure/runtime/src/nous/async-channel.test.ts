import { describe, it, expect } from "vitest";
import { AsyncChannel } from "./async-channel.js";

describe("AsyncChannel", () => {
  it("yields items pushed before iteration", async () => {
    const ch = new AsyncChannel<number>();
    ch.push(1);
    ch.push(2);
    ch.close();

    const items: number[] = [];
    for await (const item of ch) items.push(item);
    expect(items).toEqual([1, 2]);
  });

  it("yields items pushed during iteration", async () => {
    const ch = new AsyncChannel<number>();
    const items: number[] = [];

    const consumer = (async () => {
      for await (const item of ch) items.push(item);
    })();

    ch.push(1);
    // Let the microtask queue flush so the consumer picks up item 1
    await new Promise((r) => setTimeout(r, 0));
    ch.push(2);
    await new Promise((r) => setTimeout(r, 0));
    ch.close();

    await consumer;
    expect(items).toEqual([1, 2]);
  });

  it("returns empty when closed immediately", async () => {
    const ch = new AsyncChannel<string>();
    ch.close();

    const items: string[] = [];
    for await (const item of ch) items.push(item);
    expect(items).toEqual([]);
  });

  it("ignores pushes after close", async () => {
    const ch = new AsyncChannel<number>();
    ch.push(1);
    ch.close();
    ch.push(2); // should be ignored

    const items: number[] = [];
    for await (const item of ch) items.push(item);
    expect(items).toEqual([1]);
  });

  it("drains remaining items when close follows push", async () => {
    const ch = new AsyncChannel<string>();
    const items: string[] = [];

    const consumer = (async () => {
      for await (const item of ch) items.push(item);
    })();

    // Push and immediately close — items should still drain
    ch.push("a");
    ch.push("b");
    ch.close();

    await consumer;
    expect(items).toEqual(["a", "b"]);
  });

  it("handles rapid push-close-iterate sequence", async () => {
    const ch = new AsyncChannel<number>();
    for (let i = 0; i < 100; i++) ch.push(i);
    ch.close();

    const items: number[] = [];
    for await (const item of ch) items.push(item);
    expect(items).toHaveLength(100);
    expect(items[0]).toBe(0);
    expect(items[99]).toBe(99);
  });

  it("supports typed payloads", async () => {
    interface Event {
      type: string;
      data: number;
    }
    const ch = new AsyncChannel<Event>();
    ch.push({ type: "a", data: 1 });
    ch.push({ type: "b", data: 2 });
    ch.close();

    const items: Event[] = [];
    for await (const item of ch) items.push(item);
    expect(items).toEqual([
      { type: "a", data: 1 },
      { type: "b", data: 2 },
    ]);
  });

  it("consumer waits for producer", async () => {
    const ch = new AsyncChannel<number>();
    const items: number[] = [];
    const order: string[] = [];

    const consumer = (async () => {
      for await (const item of ch) {
        order.push(`recv-${item}`);
        items.push(item);
      }
      order.push("done");
    })();

    // Delay pushes to ensure consumer is awaiting
    await new Promise((r) => setTimeout(r, 10));
    order.push("push-1");
    ch.push(1);

    await new Promise((r) => setTimeout(r, 10));
    order.push("push-2");
    ch.push(2);

    await new Promise((r) => setTimeout(r, 10));
    ch.close();

    await consumer;
    expect(items).toEqual([1, 2]);
    // Consumer should have received items after they were pushed
    expect(order).toEqual(["push-1", "recv-1", "push-2", "recv-2", "done"]);
  });
});
