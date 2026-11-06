import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useState } from "react";
import type { PluginInfo } from "@/bindings";
import { SettingsFormContext, type SettingsForm } from "@/workbench/contributions";
import { createMemoryPluginsBackend, setPluginsBackend } from "../backend";
import { settingsPage, type SettingsDecl } from "../settingsPage";
import { usePlugins } from "../store";

const id = "test.ai";
const decl: SettingsDecl = {
  title: "AI",
  properties: [
    { key: "endpoint", type: "string", title: "API base URL", default: "https://old.example/v1" },
    {
      key: "model",
      type: "string",
      title: "Model",
      default: "",
      choicesCommand: "list-models",
      choicesDependencies: ["endpoint"],
    },
    { key: "promptTemplate", type: "string", title: "Prompt template", default: "Use {{language}}.", multiline: true },
  ],
  actions: [
    { command: "test-model", title: "Test selected model" },
    { command: "preview-prompt", title: "Preview prompt" },
  ],
};
const Page = settingsPage(id, decl);
let backend = createMemoryPluginsBackend();

function Form() {
  const [settings, setSettings] = useState<Record<string, Record<string, unknown>>>({});
  const form: SettingsForm = {
    scope: "global",
    useValue: () => settings,
    set: (_key, value) => setSettings(value as typeof settings),
    useExternal: () => {
      throw new Error("not used");
    },
  };
  return (
    <SettingsFormContext.Provider value={form}>
      <Page />
    </SettingsFormContext.Provider>
  );
}

beforeEach(() => {
  backend = createMemoryPluginsBackend();
  setPluginsBackend(backend);
  usePlugins.setState({
    plugins: [{ id } as PluginInfo],
    ui: [{ pluginId: id, point: "secrets", key: "api-key", decl: { name: "api-key", label: "API Key" } }],
  });
});
afterEach(() => setPluginsBackend(null));

it("shows the key on the settings page, loads models using draft values and retains manual input", async () => {
  const user = userEvent.setup();
  const run = vi.fn(async () =>
    JSON.stringify({ ok: true, models: ["model-a", "model-b"], status: 200, elapsedMs: 21 }),
  );
  backend.runCommand = run;
  render(<Form />);
  await user.type(screen.getByLabelText("Value of API Key"), "fake-key");
  await user.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(screen.getByText("Stored")).toBeInTheDocument());
  expect(screen.getByLabelText("Value of API Key")).toHaveValue("");
  fireEvent.change(screen.getByLabelText("API base URL"), { target: { value: "https://draft.example/v1" } });
  await user.click(screen.getByRole("button", { name: "Refresh models" }));
  await waitFor(() => expect(screen.getByLabelText("Available API models")).toBeEnabled());
  expect(screen.getByLabelText("Available API models")).toHaveTextContent("Choose a model");
  await user.click(screen.getByLabelText("Available API models"));
  await user.click(await screen.findByRole("option", { name: "model-b" }));
  expect(screen.getByLabelText("Model")).toHaveValue("model-b");
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "custom-model" } });
  expect(screen.getByLabelText("Model")).toHaveValue("custom-model");
  const args = (run.mock.calls as unknown[][])[0]!;
  expect(JSON.parse(args[2] as string).settings.endpoint).toBe("https://draft.example/v1");
  expect(args[2]).not.toContain("fake-key");
  expect(args[3]).toBeNull();
});

it("displays and restores the default template and previews unapplied edits", async () => {
  backend.runCommand = vi.fn(async () =>
    JSON.stringify({ ok: true, code: "promptPreview", prompt: "Custom Chinese prompt." }),
  );
  const user = userEvent.setup();
  render(<Form />);
  const prompt = screen.getByLabelText("Prompt template");
  expect(prompt.tagName).toBe("TEXTAREA");
  expect(prompt).toHaveValue("Use {{language}}.");
  fireEvent.change(prompt, { target: { value: "Custom {{language}} prompt." } });
  await user.click(screen.getByRole("button", { name: "Preview prompt" }));
  expect(await screen.findByText("Custom Chinese prompt.")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Restore default template" }));
  expect(prompt).toHaveValue("Use {{language}}.");
});

it("discards old endpoint results and coalesces clicks", async () => {
  let finish!: (value: string) => void;
  const run = vi.fn(
    () =>
      new Promise<string>((resolve) => {
        finish = resolve;
      }),
  );
  backend.runCommand = run;
  const user = userEvent.setup();
  render(<Form />);
  await user.click(screen.getByRole("button", { name: "Refresh models" }));
  expect(screen.getByRole("button", { name: "Refresh models" })).toBeDisabled();
  fireEvent.change(screen.getByLabelText("API base URL"), { target: { value: "https://new.example" } });
  await act(async () => finish(JSON.stringify({ ok: true, models: ["stale-model"] })));
  expect(screen.getByLabelText("Available API models")).toBeDisabled();
  expect(screen.queryByText("Request succeeded")).not.toBeInTheDocument();
  expect(run).toHaveBeenCalledTimes(1);
});
