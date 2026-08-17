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

/* -------------------------------------------------------------- checkbox -- */

/**
 * Selection, as distinct from state.
 *
 * A [`Switch`] says what a mod *is*; this says whether the next action applies
 * to it. They sit on the same row and must not be mistakeable for one another,
 * which is why this is a square box with a tick rather than a smaller switch.
 *
 * `indeterminate` is the "some of this group" answer a select-all box needs.
 * It is `aria-checked="mixed"` rather than a third visual state invented here,
 * because assistive technology already has a word for it.
 *
 * `onChange` takes the event's modifier keys, since a mod list wants shift-click
 * to extend a range and the alternative is every caller wiring up its own
 * listener to find that out.
 */
export function Checkbox({
  checked,
  indeterminate,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  indeterminate?: boolean;
  onChange: (v: boolean, ev: { shiftKey: boolean }) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={indeterminate ? "mixed" : checked}
      aria-label={label}
      title={label}
      disabled={disabled}
      className="checkbox"
      data-on={checked || !!indeterminate}
      onClick={(e) => onChange(!checked, { shiftKey: e.shiftKey })}
    >
      {indeterminate ? (
        <Icon.minus size={12} strokeWidth={3} />
      ) : checked ? (
        <Icon.check size={12} strokeWidth={3} />
      ) : null}
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
              /* Softer than it was, and slightly stiffer than the nav pill:
                 this one travels a shorter distance, so a stiffer spring lands
                 in about the same perceived time. */
              transition={{ type: "spring", stiffness: 420, damping: 34 }}
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

const EASE = [0.16, 1, 0.3, 1] as const;

/**
 * Standard page transition for the main content pane.
 *
 * **Enter only, and deliberately not wrapped in `AnimatePresence`.**
 *
 * It used to be asymmetric — a quick exit, an unhurried arrival — inside
 * `AnimatePresence mode="wait"`, which holds the incoming screen back until the
 * outgoing one has finished leaving. That mode has a property worth stating
 * plainly: while it is waiting, **it renders nothing at all**. So any screen
 * whose exit fails to complete does not leave a stale page behind or a visual
 * glitch; it leaves an empty window, permanently, for every screen after it.
 *
 * That happened. Settings is the only screen holding `layoutId` elements — its
 * tab pill and every `Segmented` control — and framer-motion's shared-layout
 * work on those interacts with the exit it is being asked to complete. Leaving
 * Settings emptied Mods, Downloads and Profiles until the app was restarted,
 * while Settings itself kept rendering because arriving at it was never the
 * problem.
 *
 * Keying the pane and letting React remount it removes the entire failure
 * class: there is no exit to wait on, so there is no state in which the content
 * pane is empty. The cost is the 120ms leaving fade, which nobody was looking
 * at — the arrival is the part you see, and it is unchanged.
 *
 * The travel is 8px rather than the 4px this used to be: at 4px over 180ms the
 * movement was too small and too brief to register, so switching screens looked
 * like an instant swap with a flicker rather than a transition.
 */
export const pageMotion = {
  initial: { opacity: 0, y: 8 },
  animate: {
    opacity: 1,
    y: 0,
    transition: { duration: 0.26, ease: EASE },
  },
};
