import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import {
  clearRelayApiKey,
  createCodexConfigBackup,
  deleteCodexConfigBackup,
  fetchApiModels,
  getAuthCredentialStatus,
  getDetectionPaths,
  hasUnifiedHistoryBackup,
  listCodexConfigBackups,
  restoreCodexConfigBackup,
  restoreUnifiedHistory,
} from "../lib/api";
import { defaultSettings } from "../lib/mock";
import { SettingsDrawer } from "./SettingsDrawer";

vi.mock("../lib/api", () => ({
  clearRelayApiKey: vi.fn(),
  createCodexConfigBackup: vi.fn(),
  deleteCodexConfigBackup: vi.fn(),
  fetchApiModels: vi.fn(),
  getAuthCredentialStatus: vi.fn(),
  getDetectionPaths: vi.fn(),
  hasUnifiedHistoryBackup: vi.fn(),
  listCodexConfigBackups: vi.fn(),
  restoreCodexConfigBackup: vi.fn(),
  restoreUnifiedHistory: vi.fn(),
}));

const defaultBackup = {
  id: "default-initial",
  label: "首次启动默认配置",
  createdAt: "2026-07-04T12:00:00.000Z",
  isDefault: true,
  hasConfig: true,
  hasAuth: true,
};

const manualBackup = {
  id: "manual-20260704120000123",
  label: "手动备份",
  createdAt: "2026-07-04T12:05:00.000Z",
  isDefault: false,
  hasConfig: true,
  hasAuth: true,
};

describe("SettingsDrawer", () => {
  beforeEach(() => {
    vi.mocked(getAuthCredentialStatus).mockResolvedValue({
      hasStoredOfficialAuth: true,
      hasStoredRelayApiKey: true,
      relayEndpoint: "https://api.example.com/v1",
    });
    vi.mocked(clearRelayApiKey).mockResolvedValue({
      hasStoredOfficialAuth: true,
      hasStoredRelayApiKey: false,
      relayEndpoint: null,
    });
    vi.mocked(hasUnifiedHistoryBackup).mockResolvedValue(false);
    vi.mocked(restoreUnifiedHistory).mockResolvedValue({
      restoredJsonlFiles: 0,
      restoredStateRows: 0,
      skippedReason: "no_backup_ledger",
    });
    vi.mocked(getDetectionPaths).mockResolvedValue({
      codexBinaryPath: "codex",
      codexDataDir: "~/.codex",
      stateDbPath: "~/.codex/state_5.sqlite",
      appLogDir: "logs",
    });
    vi.mocked(listCodexConfigBackups).mockResolvedValue([defaultBackup]);
    vi.mocked(createCodexConfigBackup).mockResolvedValue([defaultBackup, manualBackup]);
    vi.mocked(restoreCodexConfigBackup).mockResolvedValue([defaultBackup, manualBackup]);
    vi.mocked(deleteCodexConfigBackup).mockResolvedValue([defaultBackup]);
    vi.mocked(fetchApiModels).mockResolvedValue(["gpt-4o", "gpt-5", "o3"]);
  });

  it("hides relay-only fields in official mode", () => {
    render(<SettingsDrawer settings={defaultSettings} onClose={() => {}} onSave={() => {}} />);

    expect(screen.getByText("设置")).toBeInTheDocument();
    expect(
      screen.queryByText("接入方式、路径、刷新频率、主题与任务看板行为"),
    ).not.toBeInTheDocument();
    expect(screen.queryByLabelText("API 地址")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("API 站点名字")).not.toBeInTheDocument();
    expect(screen.queryByText("此项可选，留空即可。")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("模型名字")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("推理强度")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("速度策略")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "日志" })).not.toBeInTheDocument();
  });

  it("shows config backup controls above access mode", async () => {
    render(<SettingsDrawer settings={defaultSettings} onClose={() => {}} onSave={() => {}} />);

    expect(await screen.findByLabelText("配置备份")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /保存备份/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /恢复备份/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /删除备份/ })).toBeDisabled();
    expect(
      screen.getByLabelText("配置备份").compareDocumentPosition(screen.getByLabelText("当前模式")),
    ).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("deletes the selected manual config backup", async () => {
    vi.mocked(listCodexConfigBackups).mockResolvedValue([defaultBackup, manualBackup]);
    render(<SettingsDrawer settings={defaultSettings} onClose={() => {}} onSave={() => {}} />);

    const backupSelect = (await screen.findByLabelText("配置备份")) as HTMLSelectElement;
    fireEvent.change(backupSelect, { target: { value: manualBackup.id } });
    fireEvent.click(screen.getByRole("button", { name: /删除备份/ }));

    await waitFor(() => expect(deleteCodexConfigBackup).toHaveBeenCalledWith(manualBackup.id));
    expect(await screen.findByText("已删除所选配置备份")).toBeInTheDocument();
  });

  it("normalizes relay API endpoint to a single v1 suffix and closes after save", async () => {
    const onClose = vi.fn();
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<SettingsDrawer settings={defaultSettings} onClose={onClose} onSave={onSave} />);

    fireEvent.change(screen.getByLabelText("当前模式"), { target: { value: "relay" } });
    expect(screen.getByLabelText("API 站点名字")).toHaveValue("");
    expect(screen.getByText("此项可选，留空即可。")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("API 站点名字"), {
      target: { value: "示例站" },
    });
    const endpoint = screen.getByLabelText("API 地址") as HTMLInputElement;
    fireEvent.change(endpoint, { target: { value: "api.example.com/v1/v1/" } });
    fireEvent.blur(endpoint);

    expect(endpoint).toHaveValue("https://api.example.com/v1");

    fireEvent.click(screen.getByRole("button", { name: "保存设置" }));
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        accessMode: "relay",
        apiSiteName: "示例站",
        apiEndpoint: "https://api.example.com/v1",
      }),
    );
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("preserves relay preferences when switching back to official mode", async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(
      <SettingsDrawer
        settings={{
          ...defaultSettings,
          accessMode: "relay",
          apiSiteName: "已保存站点",
          apiEndpoint: "https://api.example.com/v1",
          apiKey: "sk-test",
          apiModel: "relay-model",
          reasoningEffort: "extreme",
          speedMode: "fast",
        }}
        onClose={() => {}}
        onSave={onSave}
      />,
    );

    fireEvent.change(screen.getByLabelText("当前模式"), { target: { value: "official" } });
    fireEvent.click(screen.getByRole("button", { name: "保存设置" }));

    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        accessMode: "official",
        apiSiteName: "已保存站点",
        apiEndpoint: "https://api.example.com/v1",
        apiKey: null,
        apiModel: "relay-model",
        reasoningEffort: "extreme",
        speedMode: "fast",
      }),
    );
  });

  it("fetches OpenAI model options while keeping manual model input editable", async () => {
    render(<SettingsDrawer settings={defaultSettings} onClose={() => {}} onSave={() => {}} />);

    fireEvent.change(screen.getByLabelText("当前模式"), { target: { value: "relay" } });
    fireEvent.change(screen.getByLabelText("API 地址"), {
      target: { value: "api.example.com/v1/v1/" },
    });
    fireEvent.change(screen.getByLabelText("API Key"), { target: { value: "sk-test" } });
    fireEvent.click(screen.getByRole("button", { name: "获取模型" }));

    await waitFor(() =>
      expect(fetchApiModels).toHaveBeenCalledWith("https://api.example.com/v1", "sk-test"),
    );
    expect(await screen.findByText("已获取 3 个 OpenAI 模型")).toBeInTheDocument();
    expect(screen.getByLabelText("模型选项")).toHaveValue("gpt-5");

    fireEvent.change(screen.getByLabelText("模型选项"), { target: { value: "gpt-4o" } });
    expect(screen.getByLabelText("模型选项")).toHaveValue("gpt-4o");

    fireEvent.change(screen.getByLabelText("模型选项"), { target: { value: "__manual__" } });
    fireEvent.change(screen.getByLabelText("手动模型名字"), {
      target: { value: "gpt-custom" },
    });
    expect(screen.getByLabelText("手动模型名字")).toHaveValue("gpt-custom");
  });

  it("shows the message from a structured Tauri save error", async () => {
    const onSave = vi.fn().mockRejectedValue({
      code: "config_error",
      message: "配置错误: API 地址已变化",
      detail: "Config(...) ",
    });
    render(<SettingsDrawer settings={defaultSettings} onClose={() => {}} onSave={onSave} />);

    fireEvent.click(screen.getByRole("button", { name: "保存设置" }));

    expect(await screen.findByText("配置错误: API 地址已变化")).toBeInTheDocument();
    expect(screen.queryByText("[object Object]")).not.toBeInTheDocument();
  });
});
