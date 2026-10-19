import type { ReactElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render as rtlRender,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { McodeProviderForm } from "@/components/providers/forms/McodeProviderForm";
import { mcodeProviderPresets } from "@/config/mcodeProviderPresets";
import { piProviderPresets } from "@/config/piProviderPresets";
import { setProviders } from "../msw/state";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    id,
    value,
    onChange,
  }: {
    id: string;
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      id={id}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const render = (ui: ReactElement) =>
  rtlRender(
    <QueryClientProvider client={createTestQueryClient()}>
      {ui}
    </QueryClientProvider>,
  );

const original = {
  kind: "custom",
  api: "anthropic-messages",
  enabled: true,
  options: {
    baseURL: "https://api.example.com/anthropic",
    apiKey: "local-test-key",
    headers: { "X-Test": "keep" },
  },
  models: {
    model: {
      name: "Model",
      limit: { context: 200000 },
      thinking: { effort: "high" },
    },
  },
  futureSetting: { keep: true },
};

describe("McodeProviderForm", () => {
  it.each([true, false])(
    "edits native fields while preserving model options and unknown settings (explicit API: %s)",
    async (explicitApi) => {
      const submit = vi.fn();
      const settings: Record<string, unknown> = { ...original };
      if (!explicitApi) delete settings.api;
      render(
        <McodeProviderForm
          appId="mcode"
          providerId="existing"
          initialData={{ name: "Existing", settingsConfig: settings }}
          submitLabel="Save"
          onSubmit={submit}
          onCancel={() => {}}
        />,
      );
      fireEvent.change(screen.getByLabelText("provider.name"), {
        target: { value: "Renamed" },
      });
      fireEvent.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
      expect(saved).toEqual({ ...settings, name: "Renamed" });
      expect(submit.mock.calls[0][0].providerKey).toBe("existing");
      expect(screen.getByLabelText("API Key")).toHaveAttribute(
        "type",
        "password",
      );
    },
  );

  it("keeps untouched extra option values verbatim when another option is edited", async () => {
    const submit = vi.fn();
    render(
      <McodeProviderForm
        appId="mcode"
        providerId="existing"
        initialData={{
          name: "Existing",
          settingsConfig: {
            kind: "custom",
            api: "anthropic-messages",
            enabled: true,
            options: {
              baseURL: "https://api.example.com/anthropic",
              apiKey: "local-test-key",
              name: "123",
              timeout: 100,
            },
            models: { model: { name: "Model" } },
          },
        }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    const valueInputs = screen.getAllByPlaceholderText("600000");
    fireEvent.change(valueInputs[valueInputs.length - 1], {
      target: { value: "200" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.options).toMatchObject({ name: "123", timeout: 200 });
  });

  it("keeps both rows with their types when a rename collides with an existing key", async () => {
    const submit = vi.fn();
    render(
      <McodeProviderForm
        appId="mcode"
        providerId="existing"
        initialData={{
          name: "Existing",
          settingsConfig: {
            kind: "custom",
            api: "anthropic-messages",
            enabled: true,
            options: {
              baseURL: "https://api.example.com/mcode",
              apiKey: "local-test-key",
              target: "100",
              source: 100,
            },
            models: { model: { name: "Model" } },
          },
        }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    const keyInputs = screen.getAllByPlaceholderText("timeout");
    fireEvent.change(keyInputs[keyInputs.length - 1], {
      target: { value: "target" },
    });
    fireEvent.blur(keyInputs[keyInputs.length - 1]);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.options).toMatchObject({ target: "100", source: 100 });
  });

  it("keeps a rejected save open and displays the failure", async () => {
    render(
      <McodeProviderForm
        appId="mcode"
        initialData={{ name: "Existing", settingsConfig: original }}
        submitLabel="Save"
        onSubmit={async () => {
          throw new Error("configuration busy");
        }}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "configuration busy",
    );
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("requires a well-formed, unused provider key for new providers", async () => {
    setProviders("mcode", {
      taken: { id: "taken", name: "Taken", settingsConfig: original },
    });
    const submit = vi.fn();
    render(
      <McodeProviderForm
        appId="mcode"
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "MiniMax" }));
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "test-key" },
    });
    const key = screen.getByLabelText(/opencode\.providerKey/);
    const save = screen.getByRole("button", { name: "Save" });
    expect(key).toHaveValue("cc-switch-mini-max");
    expect(save).toBeEnabled();

    fireEvent.change(key, { target: { value: "" } });
    expect(save).toBeDisabled();
    fireEvent.change(key, { target: { value: "-bad-" } });
    expect(screen.getByText("opencode.providerKeyInvalid")).toBeInTheDocument();
    expect(save).toBeDisabled();
    fireEvent.change(key, { target: { value: "taken" } });
    expect(
      await screen.findByText("opencode.providerKeyDuplicate"),
    ).toBeInTheDocument();
    expect(save).toBeDisabled();

    fireEvent.change(key, { target: { value: "My Key!" } });
    expect(key).toHaveValue("mykey");
    fireEvent.click(save);
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(submit.mock.calls[0][0].providerKey).toBe("mykey");
  });

  it("locks the provider key when editing", () => {
    render(
      <McodeProviderForm
        appId="mcode"
        providerId="existing"
        initialData={{ name: "Existing", settingsConfig: original }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    const key = screen.getByLabelText(/opencode\.providerKey/);
    expect(key).toHaveValue("existing");
    expect(key).toBeDisabled();
    expect(
      screen.getByText("opencode.providerKeyLockedHint"),
    ).toBeInTheDocument();
  });

  it("keeps invalid JSON drafts from breaking the structured fields", () => {
    render(
      <McodeProviderForm
        appId="mcode"
        initialData={{ name: "Existing", settingsConfig: original }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    fireEvent.change(screen.getByLabelText("provider.configJson"), {
      target: { value: '{"options":{"baseURL":42}}' },
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("provider.configJson"), {
      target: { value: JSON.stringify(original) },
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });
});
