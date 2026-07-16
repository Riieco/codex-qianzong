import { Minus, RefreshCw, Settings, ShieldCheck, X } from "lucide-react";
import { useRef } from "react";
import type { MouseEvent, PointerEvent } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { formatTime } from "../lib/format";
import type { UsageSnapshot } from "../types/usage";

interface HeaderBarProps {
  snapshot: UsageSnapshot | null;
  isRefreshing: boolean;
  onRefresh: () => void;
  onOpenSettings: () => void;
}

export function HeaderBar({ snapshot, isRefreshing, onRefresh, onOpenSettings }: HeaderBarProps) {
  const plan = formatPlan(snapshot?.account?.planType ?? snapshot?.account?.accountType);
  const dragOrigin = useRef<{ pointerId: number; x: number; y: number } | null>(null);

  function handlePointerDown(event: PointerEvent<HTMLElement>) {
    if (event.button !== 0 || event.detail > 1 || !isTauriRuntime()) return;
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;
    if (target.closest("button, a, input, select, textarea")) return;

    event.preventDefault();
    dragOrigin.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY };
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function handlePointerMove(event: PointerEvent<HTMLElement>) {
    const origin = dragOrigin.current;
    if (!origin || origin.pointerId !== event.pointerId) return;
    if (Math.hypot(event.clientX - origin.x, event.clientY - origin.y) < 5) return;

    dragOrigin.current = null;
    releasePointerCapture(event);
    void getCurrentWindow()
      .startDragging()
      .catch(() => undefined);
  }

  function clearDragOrigin(event: PointerEvent<HTMLElement>) {
    if (dragOrigin.current?.pointerId !== event.pointerId) return;
    dragOrigin.current = null;
    releasePointerCapture(event);
  }

  return (
    <header
      className="header-bar"
      onDoubleClick={blockHeaderDoubleClick}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={clearDragOrigin}
      onPointerCancel={clearDragOrigin}
    >
      <div className="brand-lockup">
        <div className="brand-mark" aria-hidden="true">
          Q
        </div>
        <div>
          <h1>codex-qianzong</h1>
          <p>Codex 额度、令牌与任务遥测</p>
        </div>
      </div>

      <div className="header-status" aria-label="应用状态">
        <span className="status-pill">
          <ShieldCheck size={14} />
          {plan}
        </span>
        <span className="muted">上次同步 {formatTime(snapshot?.refreshedAt)}</span>
        <button className="icon-button" onClick={onRefresh} aria-label="刷新使用快照">
          <RefreshCw size={16} className={isRefreshing ? "spin" : ""} />
        </button>
        <button className="icon-button" onClick={onOpenSettings} aria-label="打开设置">
          <Settings size={16} />
        </button>
        <div className="window-controls" aria-label="窗口控制">
          <button
            className="icon-button window-control"
            onClick={minimizeWindow}
            aria-label="最小化窗口"
          >
            <Minus size={15} />
          </button>
          <button
            className="icon-button window-control close"
            onClick={closeWindow}
            aria-label="关闭窗口"
          >
            <X size={15} />
          </button>
        </div>
      </div>
    </header>
  );
}

function releasePointerCapture(event: PointerEvent<HTMLElement>) {
  if (event.currentTarget.hasPointerCapture(event.pointerId)) {
    event.currentTarget.releasePointerCapture(event.pointerId);
  }
}

function blockHeaderDoubleClick(event: MouseEvent<HTMLElement>) {
  event.preventDefault();
  event.stopPropagation();
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function minimizeWindow() {
  if (!isTauriRuntime()) return;
  void getCurrentWindow()
    .minimize()
    .catch(() => undefined);
}

function closeWindow() {
  if (!isTauriRuntime()) return;
  void getCurrentWindow()
    .close()
    .catch(() => undefined);
}

function formatPlan(plan?: string | null): string {
  if (!plan) return "离线";
  const normalized = plan.toLowerCase();
  if (normalized === "chatgpt") return "ChatGPT";
  if (normalized === "pro") return "Pro 计划";
  if (normalized === "plus") return "Plus 计划";
  return `${plan} 计划`;
}
