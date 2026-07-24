/** Shared primitives: switch, segmented control, chips, toasts, spinner. */

import { AnimatePresence, motion } from "framer-motion";
import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { Icon } from "./icons";

export { Icon, Logo } from "./icons";
export type { IconName } from "./icons";

/* ---------------------------------------------------------------- switch -- */

export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className="switch"
      data-on={checked}
      onClick={() => onChange(!checked)}
      style={disabled ? { opacity: 0.4, cursor: "not-allowed" } : undefined}
    >
      <motion.span
        className="switch-thumb"
        animate={{ x: checked ? 16 : 0 }}
        transition={{ type: "spring", stiffness: 700, damping: 40 }}
      />
    </button>
  );
}

/* ------------------------------------------------------ segmented control -- */

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  idPrefix,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  idPrefix: string;
}) {
  return (
    <div className="segmented" role="tablist">
      {options.map((o) => (
        <button
          key={o.value}
          role="tab"
          aria-selected={value === o.value}
          className={value === o.value ? "active" : ""}
          onClick={() => onChange(o.value)}
        >
          {value === o.value && (
            <motion.span
              layoutId={`${idPrefix}-seg-pill`}
              className="seg-pill"
              transition={{ type: "spring", stiffness: 500, damping: 40 }}
            />
          )}
          <span>{o.label}</span>
        </button>
      ))}
    </div>
  );
}

/* ---------------------------------------------------------------- toasts -- */

type Toast = { id: number; message: string; kind: "ok" | "bad" | "info" };

const ToastContext = createContext<{
  push: (message: string, kind?: Toast["kind"]) => void;
} | null>(null);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);

  const push = useCallback((message: string, kind: Toast["kind"] = "info") => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t, { id, message, kind }]);
    setTimeout(
      () => setToasts((t) => t.filter((x) => x.id !== id)),
      kind === "bad" ? 7000 : 4000,
    );
  }, []);

  const value = useMemo(() => ({ push }), [push]);

  return (
    <ToastContext.Provider value={value}>
      {children}
      <div className="toast-wrap">
        <AnimatePresence mode="popLayout">
          {toasts.map((t) => (
            <motion.div
              key={t.id}
              layout
              className={`toast ${t.kind}`}
              initial={{ opacity: 0, y: 16, scale: 0.96 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, x: 24, scale: 0.96 }}
              transition={{ type: "spring", stiffness: 460, damping: 34 }}
            >
              <span
                style={{
                  color:
                    t.kind === "ok"
                      ? "var(--success)"
                      : t.kind === "bad"
                        ? "var(--danger)"
                        : "var(--accent)",
                  display: "grid",
                  placeItems: "center",
                }}
              >
                {t.kind === "ok" ? (
                  <Icon.check />
                ) : t.kind === "bad" ? (
                  <Icon.warning />
                ) : (
                  <Icon.info />
                )}
              </span>
              <span>{t.message}</span>
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
    </ToastContext.Provider>
  );
}

export function useToast() {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToast must be used inside ToastProvider");
  return ctx;
}

/* ------------------------------------------------------------------ misc -- */

export function Chip({
  children,
  kind = "default",
}: {
  children: ReactNode;
  kind?: "default" | "ok" | "warn" | "bad" | "accent";
}) {
  return <span className={`chip ${kind === "default" ? "" : kind}`}>{children}</span>;
}

export function Spinner() {
  return <span className="spinner" aria-label="Loading" />;
}

/** Standard page transition for the main content pane. */
export const pageMotion = {
  initial: { opacity: 0, y: 4 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -4 },
  transition: { duration: 0.18, ease: [0.16, 1, 0.3, 1] as const },
};
