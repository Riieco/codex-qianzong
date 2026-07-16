import { formatError } from "./errors";

describe("formatError", () => {
  it("reads structured Tauri command errors", () => {
    expect(
      formatError({
        code: "config_error",
        message: "配置错误: 未找到已保存的官方登录凭据",
        detail: "Config(...) ",
      }),
    ).toBe("配置错误: 未找到已保存的官方登录凭据");
  });

  it("keeps native and string errors readable", () => {
    expect(formatError(new Error("native failure"))).toBe("native failure");
    expect(formatError("plain failure")).toBe("plain failure");
  });
});
