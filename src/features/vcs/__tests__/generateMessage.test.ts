import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useProject } from "@/platform/project";
import { registerCommitMessageGenerator } from "@/workbench/contributions";
import { useCommit } from "../commitStore";
import { generateCommitMessage, useMessageGeneration } from "../generateMessage";
import { useVcs } from "../store";

let dispose: (() => void) | undefined;
beforeEach(() => {
  useMessageGeneration.setState({ busy: false });
  useCommit.setState({ message: "my draft", focusTick: 0 });
  useVcs.setState({ repo: "/p", root: "/p", version: 1 });
});
afterEach(() => dispose?.());

it("fills the draft from the selected generator without staging or committing", async () => {
  const generate = vi.fn(async () => "feat: add a switch");
  dispose = registerCommitMessageGenerator({ id: "test", title: "Test", generate });
  await generateCommitMessage("test");
  expect(generate).toHaveBeenCalledWith("/p");
  expect(useCommit.getState().message).toBe("feat: add a switch");
  expect(useCommit.getState().focusTick).toBe(1);
  expect(useMessageGeneration.getState().busy).toBe(false);
});

it.each(["draft", "repo", "project", "status"])("discards a late result after the %s changes", async (change) => {
  let finish!: (message: string) => void;
  dispose = registerCommitMessageGenerator({
    id: "test",
    title: "Test",
    generate: () =>
      new Promise((r) => {
        finish = r;
      }),
  });
  const pending = generateCommitMessage("test");
  if (change === "draft") useCommit.setState({ message: "new draft" });
  if (change === "repo") useVcs.setState({ repo: "/other" });
  if (change === "project") useProject.setState({ project: { ...useProject.getState().project!, root: "/other" } });
  if (change === "status") useVcs.setState({ version: 2 });
  finish("stale message");
  await pending;
  expect(useCommit.getState().message).toBe(change === "draft" ? "new draft" : "my draft");
});

it("coalesces clicks and keeps the draft after failures", async () => {
  let fail!: (error: Error) => void;
  const generate = vi.fn(
    () =>
      new Promise<string>((_, reject) => {
        fail = reject;
      }),
  );
  dispose = registerCommitMessageGenerator({ id: "test", title: "Test", generate });
  const pending = generateCommitMessage("test");
  await generateCommitMessage("test");
  expect(generate).toHaveBeenCalledTimes(1);
  fail(new Error("offline"));
  await pending;
  expect(useCommit.getState().message).toBe("my draft");
  expect(useMessageGeneration.getState().busy).toBe(false);
});
