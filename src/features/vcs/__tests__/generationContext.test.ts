import { beforeEach, expect, it, vi } from "vitest";

const directory = vi.fn();
const probe = vi.fn();
const read = vi.fn();
vi.mock("@/platform/ipc", async (original) => {
  const real = await original<typeof import("@/platform/ipc")>();
  return {
    ...real,
    commands: {
      ...real.commands,
      fsReadDir: (path: string) => directory(path),
      fsProbe: (path: string) => probe(path),
      fsReadText: (path: string) => read(path),
    },
  };
});
import { selectedGenerationContext } from "../generationContext";
import { useCommit } from "../commitStore";
import { useVcs } from "../store";

beforeEach(() => {
  directory.mockReset().mockResolvedValue({ status: "ok", data: [] });
  probe.mockReset().mockResolvedValue({ status: "ok", data: { size: 20, binary: false, tooLarge: false } });
  read.mockReset().mockResolvedValue({ status: "ok", data: "export const added = true;\n" });
  useVcs.setState({
    root: "/p",
    repo: "/p",
    roots: ["/p"],
    status: [
      { path: "/p/new.ts", relPath: "new.ts", staged: null, unstaged: "untracked", renamedFrom: null },
      { path: "/p/.env", relPath: ".env", staged: null, unstaged: "untracked", renamedFrom: null },
    ],
  });
  useCommit.setState({
    checks: new Map([
      ["/p/new.ts", "all"],
      ["/p/.env", "all"],
    ]),
    excludedHunks: new Map(),
  });
});

it("captures included untracked text without staging and never reads secret files", async () => {
  const selection = await selectedGenerationContext();
  expect(selection.files).toEqual([
    { path: "new.ts", repo: "/p", text: "export const added = true;\n", staged: false, deleted: false, omitted: false },
  ]);
  expect(probe).toHaveBeenCalledTimes(1);
  expect(read).toHaveBeenCalledWith("/p/new.ts");
});

it("omits binary and large contents while preserving their names", async () => {
  probe.mockResolvedValue({ status: "ok", data: { size: 10_000_000, binary: true, tooLarge: false } });
  const selection = await selectedGenerationContext();
  expect(selection.files[0]).toMatchObject({ path: "new.ts", omitted: true });
  expect(selection.files[0]!.text).toBeUndefined();
  expect(read).not.toHaveBeenCalled();
});

it("respects unchecked files and fails clearly for an empty selection", async () => {
  useCommit.setState({
    checks: new Map([
      ["/p/new.ts", "none"],
      ["/p/.env", "none"],
    ]),
  });
  await expect(selectedGenerationContext()).rejects.toThrow("Select some non-sensitive changes");
  expect(read).not.toHaveBeenCalled();
});

it("does not read the working copy for a staged-only file", async () => {
  useVcs.setState({
    status: [{ path: "/p/new.ts", relPath: "new.ts", staged: "added", unstaged: "modified", renamedFrom: null }],
  });
  useCommit.setState({ checks: new Map([["/p/new.ts", "staged"]]) });
  expect((await selectedGenerationContext()).files[0]).toMatchObject({ staged: true });
  expect(probe).not.toHaveBeenCalled();
  expect(read).not.toHaveBeenCalled();
});

it("never dereferences symlinks into file content", async () => {
  directory.mockResolvedValue({ status: "ok", data: [{ path: "/p/new.ts", kind: "symlink" }] });
  expect((await selectedGenerationContext()).files[0]).toMatchObject({ omitted: true });
  expect(read).not.toHaveBeenCalled();
  expect(probe).not.toHaveBeenCalled();
});
