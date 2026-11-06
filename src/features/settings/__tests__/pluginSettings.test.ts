import { expect, it } from "vitest";
import { validateSettings } from "../model/schema";
import { openSettings, useSettingsDialog } from "../model/staging";
import { useProject } from "@/platform/project";

it("preserves plugin IDs containing dots and scalar settings through dialog validation", () => {
  const plugins = {
    "finalpi.commit-assistant": {
      endpoint: "https://example.test/v1",
      model: "custom",
      promptTemplate: "Use {{language}}",
      enabled: false,
      limit: 0,
    },
  };
  expect(validateSettings({ plugins }).plugins).toEqual(plugins);
  useProject.setState({ project: null, settings: { plugins } as never });
  openSettings("plugins.finalpi-commit-assistant");
  expect(useSettingsDialog.getState().base.plugins).toEqual(plugins);
  expect(useSettingsDialog.getState().globalBase.plugins).toEqual(plugins);
  useSettingsDialog.setState({ open: false });
});
