import { afterEach, expect, it, vi } from "vitest";
import { useDatabase, watchIdleConnections } from "./store";

let stop: (() => void) | undefined;
afterEach(() => {
  stop?.();
  vi.useRealTimers();
});

it("has no interval without eligible connections and starts/stops with their lifetime", () => {
  vi.useFakeTimers();
  useDatabase.setState({ sources: [], connected: new Set() });
  stop = watchIdleConnections();
  expect(vi.getTimerCount()).toBe(0);
  useDatabase.setState({ sources: [{ id: "db", options: { autoDisconnectSecs: 60 } }] });
  expect(vi.getTimerCount()).toBe(0);
  useDatabase.setState({ connected: new Set(["db"]) });
  expect(vi.getTimerCount()).toBe(1);
  useDatabase.setState({ connected: new Set() });
  expect(vi.getTimerCount()).toBe(0);
  useDatabase.setState({ connected: new Set(["db"]), sources: [{ id: "db" }] });
  expect(vi.getTimerCount()).toBe(0);
});
