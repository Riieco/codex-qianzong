interface CommandErrorPayload {
  message?: unknown;
  detail?: unknown;
  code?: unknown;
}

export function formatError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (isCommandError(error) && typeof error.message === "string") return error.message;

  try {
    const serialized = JSON.stringify(error);
    if (serialized && serialized !== "{}") return serialized;
  } catch {
    // Fall through to the stable generic message.
  }
  return "发生未知错误";
}

function isCommandError(error: unknown): error is CommandErrorPayload {
  return typeof error === "object" && error !== null;
}
